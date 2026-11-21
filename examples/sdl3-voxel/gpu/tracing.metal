struct Hit { int3 cell; float3 position; float3 normal; float distance; uint material; uint status; };

// Status 0 means the segment exited without a hit; 2 means an exhausted budget.
// Empty bricks are skipped exactly. Occupied bricks use voxel boundary DDA.
inline Hit trace_voxels(float3 origin, float3 direction, float maximum,
    constant Params& p, device uint* cells, device atomic_uint* parents, device uint* bricks) {
    Hit hit; hit.cell=int3(0); hit.position=origin; hit.normal=float3(0,1,0); hit.distance=0; hit.material=0; hit.status=0;
    float3 inv=1.0f/select(direction,float3(1e-20f),abs(direction)<1e-20f);
    float3 t0=(float3(0)-origin)*inv, t1=(float3(p.world.xyz)-origin)*inv;
    float3 near3=min(t0,t1), far3=max(t0,t1);
    float enter=max(max(near3.x,near3.y),near3.z);
    float leave=min(maximum,min(min(far3.x,far3.y),far3.z));
    if (leave<max(enter,0.0f)) return hit;
    float t=max(enter,0.0f);
    int3 c=clamp(int3(floor(origin+direction*t)),int3(0),int3(p.world.xyz)-1);
    uint axis=near3.x>near3.y ? 0u:1u; axis=near3[axis]>near3.z ? axis:2u;
    hit.normal=float3(0); hit.normal[axis]=direction[axis]>0 ? -1.0f:1.0f;
    for (uint iteration=0;iteration<1024;iteration++) {
        if(t>=leave) { hit.distance=leave; return hit; }
        float3 position=origin+direction*t;
        if(!inside(c,p)) { hit.distance=t; return hit; }
        uint m=material(c,p,cells,parents);
        if(m) { hit.cell=c; hit.position=position; hit.distance=t; hit.material=m; hit.status=1; return hit; }
        uint brick=index3(uint3(c)/8u,p.config.x);
        float size=bricks[brick]==0u ? 8.0f:1.0f;
        float3 base=floor(float3(c)/size)*size;
        float3 boundary=base+select(float3(0),float3(size),direction>0);
        float3 crossing=(boundary-origin)*inv;
        crossing=select(crossing,float3(INFINITY),abs(direction)<1e-20f);
        axis=crossing.x<crossing.y ? 0u:1u; axis=crossing[axis]<crossing.z ? axis:2u;
        // Reprojection can round an unselected coordinate across a plane.
        // Consume that already-crossed plane at the current ray time, then
        // test the entered cell. Never move backward or turn this into a miss.
        float next=max(crossing[axis],t);
        int3 next_cell=int3(floor(origin+direction*next));
        // Advance all exactly tied axes. No geometric epsilon can skip a sliver.
        for(uint a=0;a<3;a++) {
            if(crossing[a]<=next) next_cell[a]=int(boundary[a])+(direction[a]<0 ? -1:0);
            else next_cell[a]=clamp(next_cell[a],int(base[a]),int(base[a]+size)-1);
        }
        c=next_cell; t=next;
        hit.normal=float3(0); hit.normal[axis]=direction[axis]>0 ? -1.0f:1.0f;
    }
    hit.status=2; hit.position=origin+direction*t; hit.distance=t;
    return hit;
}
// Visibility queries do not need hit positions, normals or material payloads.
// Keeping their live state compact reduces register pressure in the image pass.
inline uint trace_visibility(float3 origin, float3 direction, float maximum,
    constant Params& p, device uint* cells, device atomic_uint* parents, device uint* bricks) {
    float3 inv=1.0f/select(direction,float3(1e-20f),abs(direction)<1e-20f);
    float3 t0=(float3(0)-origin)*inv, t1=(float3(p.world.xyz)-origin)*inv;
    float3 near3=min(t0,t1), far3=max(t0,t1);
    float enter=max(max(near3.x,near3.y),near3.z);
    float leave=min(maximum,min(min(far3.x,far3.y),far3.z));
    if (leave<max(enter,0.0f)) return 0u;
    float t=max(enter,0.0f);
    int3 c=clamp(int3(floor(origin+direction*t)),int3(0),int3(p.world.xyz)-1);
    uint axis=near3.x>near3.y ? 0u:1u; axis=near3[axis]>near3.z ? axis:2u;
    for (uint iteration=0;iteration<1024;iteration++) {
        if(t>=leave) return 0u;
        if(!inside(c,p)) return 0u;
        uint m=material(c,p,cells,parents);
        if(m) return 1u;
        uint brick=index3(uint3(c)/8u,p.config.x);
        float size=bricks[brick]==0u ? 8.0f:1.0f;
        float3 base=floor(float3(c)/size)*size;
        float3 boundary=base+select(float3(0),float3(size),direction>0);
        float3 crossing=(boundary-origin)*inv;
        crossing=select(crossing,float3(INFINITY),abs(direction)<1e-20f);
        axis=crossing.x<crossing.y ? 0u:1u; axis=crossing[axis]<crossing.z ? axis:2u;
        // Reprojection can round an unselected coordinate across a plane.
        // Consume that already-crossed plane at the current ray time, then
        // test the entered cell. Never move backward or turn this into a miss.
        float next=max(crossing[axis],t);
        int3 next_cell=int3(floor(origin+direction*next));
        // Advance all exactly tied axes. No geometric epsilon can skip a sliver.
        for(uint a=0;a<3;a++) {
            if(crossing[a]<=next) next_cell[a]=int(boundary[a])+(direction[a]<0 ? -1:0);
            else next_cell[a]=clamp(next_cell[a],int(base[a]),int(base[a]+size)-1);
        }
        c=next_cell; t=next;
    }
    return 2u;
}
// The exact signed grid field supplies a conservative Euclidean lower bound.
// Subtract both half-cell diagonals for the off-center sample and solid cube.
// Near surfaces, exact occupancy DDA resolves the hit and normal.
inline Hit trace_indirect(float3 origin,float3 direction,float maximum,
    constant Params& p,device uint* cells,device atomic_uint* parents,
    device uint* bricks,device uint* field) {
    float traveled=0;
    for(uint step=0;step<256;step++) {
        float3 position=origin+direction*traveled; int3 c=int3(floor(position));
        if(!inside(c,p)||traveled>=maximum) {
            Hit h; h.cell=int3(0); h.position=position; h.normal=float3(0,1,0);
            h.distance=traveled; h.material=0; h.status=0; return h;
        }
        uint packed=field[index3(uint3(c),p.world.x)];
        float signed_grid=material(c,p,cells,parents)!=0u ? -float(packed>>16):float(packed&65535u);
        float bound=signed_grid*.577350269f-1.732050808f;
        if(bound<.75f) {
            Hit h=trace_voxels(position,direction,maximum-traveled,p,cells,parents,bricks);
            h.distance+=traveled; return h;
        }
        traveled+=min(bound*.95f,maximum-traveled);
    }
    // A long grazing ray can consume the SDF work budget without nearing a
    // surface. Complete its remaining segment with exact hierarchy/DDA.
    Hit h=trace_voxels(origin+direction*traveled,direction,maximum-traveled,p,cells,parents,bricks);
    h.distance+=traveled; return h;
}
inline float3 surface_color(uint m,float3 position) {
    float3 color=m==1u ? float3(.23,.27,.29):m==2u ? float3(.70,.25,.11):
        m==3u ? float3(.34,.21,.10):m==4u ? float3(1,.63,.20):
        m==5u ? float3(.16,.21,.24):m==6u ? float3(.85,.78,.63):
        m==7u ? float3(.24,.48,.52):float3(.36,.44,.30);
    uint3 c=uint3(max(floor(position),float3(0)));
    float variation=.90f+.18f*random01(c.x+7919u*c.y+104729u*c.z);
    if(m==2u && ((c.y%3u)==0u || ((c.x+(c.y/3u%2u)*3u)%7u)==0u)) variation*=.76f;
    return color*variation;
}
inline float3 sky_color(float3 direction) {
    float elevation=clamp(direction.y*.5f+.5f,0.0f,1.0f);
    return mix(float3(.72,.78,.81),float3(.24,.45,.72),pow(elevation,.65f))*.65f;
}
inline float3 sun_direction() { return normalize(float3(.48,.82,.31)); }
inline float visible_sun(float3 position,constant Params& p,device uint* cells,
    device atomic_uint* parents,device uint* bricks,device atomic_uint* stats) {
    uint status=trace_visibility(position,sun_direction(),float(p.world.x)*2,p,cells,parents,bricks);
    if(status==2u) atomic_fetch_add_explicit(stats+7,1u,memory_order_relaxed);
    return status==0u ? 1.0f:0.0f;
}
