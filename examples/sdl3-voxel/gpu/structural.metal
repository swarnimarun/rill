// Nine rigid building assemblies with destructible foundation spring joints.
// Rest materials live in cells[N^3..2*N^3); transformed voxels use the same
// display overlay as loose debris. The rest atlas is the mass authority.

inline uint structure_index(uint3 c,uint n) {
    uint tile=n/3u;
    return c.x/tile+3u*(c.z/tile);
}
inline uint structure_story(uint n) { return max(5u,n/12u); }
inline uint structure_half(uint n) { return max(2u,n/64u); }
inline bool broken_anchor(Structure state,uint index) {
    return index<32u ? (state.info.z&(1u<<index))!=0u : (state.info.w&(1u<<(index-32u)))!=0u;
}
inline void break_anchor(thread Structure& state,uint index) {
    if(index<32u) state.info.z|=1u<<index;
    else state.info.w|=1u<<(index-32u);
}
inline bool foundation_column(uint x,uint z,constant Params& p,device uint* cells) {
    for(uint y=1u;y<structure_story(p.world.x);y++)
        if(cells[index3(uint3(x,y,z),p.world.x)]==0u) return false;
    return true;
}

kernel void structure_init(RESOURCES,uint id [[thread_position_in_grid]]) {
    if(id>=volume(p)) return;
    uint3 c=coordinate(id,p.world.x);
    uint tile=p.world.x/3u;
    bool building=c.x<tile*3u && c.z<tile*3u && c.y>=structure_story(p.world.x);
    cells[volume(p)+id]=building ? cells[id]:0u;
    if(building) cells[id]=0u;
    if(id<9u) {
        Structure state;
        state.rest=state.position=state.velocity=state.angle=state.spin=float4(0);
        state.bounds_min=state.bounds_max=float4(0);
        state.info=uint4(0);
        structures(bodies,p)[id]=state;
    }
    if(id==0u) {
        atomic_store_explicit(stats+12,1u,memory_order_relaxed);
        atomic_store_explicit(stats+17,1u,memory_order_relaxed);
        atomic_store_explicit(stats+21,1u,memory_order_relaxed);
        atomic_fetch_or_explicit(stats+18,1u,memory_order_relaxed);
    }
}

// One 64-lane group reduces each building's integer mass, first moments and
// bounds. The resulting integers match a serial scan before conversion to float.
kernel void structure_analyze(RESOURCES,uint3 group [[threadgroup_position_in_grid]],
    uint lane [[thread_index_in_threadgroup]]) {
    uint id=group.x;
    if(id>=9u || atomic_load_explicit(stats+12,memory_order_relaxed)==0u) return;
    if(structure_materials_unchanged(stats)) return;
    uint n=p.world.x,tile=n/3u;
    uint2 block=uint2(id%3u,id/3u);
    uint x0=block.x*tile,z0=block.y*tile,base=structure_story(n);
    uint top=base+n/3u+(hash32(id)%3u)*base;
    uint4 moments=uint4(0);
    uint3 low=uint3(n),high=uint3(0);
    uint height=top-base,count=tile*height*tile;
    for(uint index=lane;index<count;index+=64u) {
        uint3 c(x0+index%tile,base+(index/tile)%height,z0+index/(tile*height));
        if(cells[volume(p)+index3(c,n)]==0u) continue;
        moments+=uint4(1u,c);
        low=min(low,c); high=max(high,c+1u);
    }
    threadgroup uint4 partial_moments[64];
    threadgroup uint3 partial_low[64],partial_high[64];
    partial_moments[lane]=moments;
    partial_low[lane]=low;
    partial_high[lane]=high;
    threadgroup_barrier(mem_flags::mem_threadgroup);
    for(uint stride=32u;stride>0u;stride/=2u) {
        if(lane<stride) {
            partial_moments[lane]+=partial_moments[lane+stride];
            partial_low[lane]=min(partial_low[lane],partial_low[lane+stride]);
            partial_high[lane]=max(partial_high[lane],partial_high[lane+stride]);
        }
        threadgroup_barrier(mem_flags::mem_threadgroup);
    }
    if(lane!=0u) return;
    if(id==0u) { atomic_store_explicit(stats+17,1u,memory_order_relaxed); atomic_store_explicit(stats+21,1u,memory_order_relaxed); }
    uint mass=partial_moments[0].x;
    low=partial_low[0]; high=partial_high[0];
    Structure state=structures(bodies,p)[id];
    if(mass>state.info.x) atomic_fetch_add_explicit(stats+14,mass-state.info.x,memory_order_relaxed);
    else if(mass<state.info.x) atomic_fetch_sub_explicit(stats+14,state.info.x-mass,memory_order_relaxed);
    if(mass!=0u && state.info.y==0u) atomic_fetch_add_explicit(stats+13,1u,memory_order_relaxed);
    if(mass==0u && state.info.y!=0u) atomic_fetch_sub_explicit(stats+13,1u,memory_order_relaxed);
    if(mass==0u) {
        state.info.xy=uint2(0);
        atomic_fetch_and_explicit(stats+16,~(1u<<id),memory_order_relaxed);
        structures(bodies,p)[id]=state;
        return;
    }
    float3 center=float3(partial_moments[0].yzw)/float(mass)+0.5f;
    if(state.rest.w==0.0f) {
        // Spare w lanes retain the initial horizontal load center. Later
        // asymmetric material removal can then change the tipping moment.
        state.position=float4(center,center.x);
        state.velocity.w=center.z;
        state.rest=float4(center,float(mass));
        uint width=2u*structure_half(n)-1u;
        state.bounds_min.w=float(width*width);
    } else {
        // Changing the rest COM must leave every surviving world-space voxel
        // at the same position and retain the existing assembly orientation.
        state.position.xyz+=rotate_xyz(center-state.rest.xyz,state.angle.xyz);
        state.rest.xyz=center;
    }
    state.info.x=mass; state.info.y=1u;
    state.bounds_min.xyz=float3(low); state.bounds_max.xyz=float3(high);
    uint radius_cells=structure_half(n),width=2u*radius_cells-1u,anchors=0u;
    uint cx=x0+tile/2u,cz=z0+tile/2u;
    for(uint a=0u;a<width*width;a++) {
        uint x=cx+a%width-(radius_cells-1u),z=cz+a/width-(radius_cells-1u);
        if(!broken_anchor(state,a) && foundation_column(x,z,p,cells)) ++anchors;
    }
    state.bounds_max.w=float(anchors);
    structures(bodies,p)[id]=state;
}

inline float3 structure_inertia(Structure state) {
    float3 size=max(float3(1),state.bounds_max.xyz-state.bounds_min.xyz);
    return float(state.info.x)/12.0f*float3(size.y*size.y+size.z*size.z,size.x*size.x+size.z*size.z,size.x*size.x+size.y*size.y);
}
inline float lowest_corner(Structure state) {
    float minimum=INFINITY;
    for(uint corner=0u;corner<8u;corner++) {
        float3 position=select(state.bounds_min.xyz,state.bounds_max.xyz,bool3(corner&1u,corner&2u,corner&4u));
        minimum=min(minimum,state.position.y+rotate_xyz(position-state.rest.xyz,state.angle.xyz).y);
    }
    return minimum;
}

kernel void structure_integrate(RESOURCES,uint id [[thread_position_in_grid]]) {
    if(id>=9u || atomic_load_explicit(stats+12,memory_order_relaxed)==0u) return;
    Structure state=structures(bodies,p)[id];
    if(state.info.y==0u || state.info.x==0u) return;
    // An untouched building begins in exact spring equilibrium. Sleeping that
    // state prevents floating-point noise from invalidating a static city.
    // Analyze recomputes mass and anchors after edits, waking damaged buildings.
    if(float(state.info.x)==state.rest.w && state.bounds_max.w==state.bounds_min.w &&
       state.info.z==0u && state.info.w==0u && all(state.position.xyz==state.rest.xyz) &&
       all(state.angle.xyz==float3(0)) && all(state.velocity.xyz==float3(0)) && all(state.spin.xyz==float3(0))) return;
    float3 old_position=state.position.xyz,old_angle=state.angle.xyz;
    float dt=clamp(p.dynamics.x,0.0f,0.0333334f)/4.0f;
    float mass=float(state.info.x),gravity=max(p.dynamics.y,0.0f);
    float3 inertia=max(structure_inertia(state),float3(1));
    uint tile=p.world.x/3u ,radius_cells=structure_half(p.world.x),width=2u*radius_cells-1u;
    uint cx=(id%3u)*tile+tile/2u,cz=(id/3u)*tile+tile/2u;
    float initial=max(1.0f,state.bounds_min.w);
    float stiffness=mass*64.0f/initial,damping=mass*11.2f/initial;
    float preload=mass*gravity/initial;
    for(uint step=0u;step<4u;step++) {
        float3 force=float3(0,-mass*gravity,0),torque=float3(0);
        uint supported=0u;
        for(uint a=0u;a<width*width;a++) {
            uint x=cx+a%width-(radius_cells-1u),z=cz+a/width-(radius_cells-1u);
            if(broken_anchor(state,a) || !foundation_column(x,z,p,cells)) continue;
            float3 anchor=float3(float(x)+0.5f,float(structure_story(p.world.x))+0.5f,float(z)+0.5f);
            float3 arm=rotate_xyz(anchor-state.rest.xyz,state.angle.xyz);
            float3 delta=state.position.xyz+arm-anchor;
            if(length(delta)>max(2.0f,float(p.world.x)/64.0f)) { break_anchor(state,a); continue; }
            float3 speed=state.velocity.xyz+cross(state.spin.xyz,arm);
            // Foundation members carry vertical spring loads. Their bending
            // resistance is the angular joint below, rather than hidden rigid
            // pins that would prevent a damaged building from leaning.
            float variance=max(1.0f/12.0f,(float(width*width)-1.0f)/12.0f);
            float2 initial_offset=float2(state.position.w-float(cx)-0.5f,state.velocity.w-float(cz)-0.5f);
            float2 anchor_offset=float2(float(x)-float(cx),float(z)-float(cz));
            // Separable weights balance both first moments without clipping
            // a diagonal corner's load and accidentally increasing total lift.
            float2 weights=max(float2(0),1.0f+initial_offset*anchor_offset/variance);
            float load_weight=weights.x*weights.y;
            float support=clamp(preload*load_weight-stiffness*delta.y-damping*speed.y,-preload*4.0f,preload*6.0f);
            float3 reaction(0,support,0);
            force+=reaction; torque+=cross(arm,reaction); ++supported;
        }
        float fraction=float(supported)/initial;
        float lever=max(1.0f,state.rest.y-float(structure_story(p.world.x)));
        float bend=mass*gravity*lever*2.0f*fraction*fraction*fraction;
        torque.xz-=state.angle.xz*bend;
        torque.y-=state.angle.y*bend*0.2f;
        torque-=state.spin.xyz*inertia*0.9f;
        // A weak translational joint keeps an intact foundation centered while
        // asymmetric vertical reactions supply the actual tipping torque.
        force.xz-=(state.position.xz-state.rest.xz)*mass*3.0f*fraction;
        force.xz-=state.velocity.xz*mass*1.2f*fraction;
        state.velocity.xyz+=force/mass*dt;
        state.spin.xyz+=torque/inertia*dt;
        state.velocity.xyz=clamp(state.velocity.xyz,float3(-30),float3(30));
        state.spin.xyz=clamp(state.spin.xyz,float3(-1.5f),float3(1.5f));
        state.position.xyz+=state.velocity.xyz*dt;
        state.angle.xyz+=state.spin.xyz*dt;
        float bottom=lowest_corner(state);
        if(bottom<1.0f) {
            state.position.y+=1.0f-bottom;
            if(state.velocity.y<0.0f) state.velocity.y*=-0.08f;
            state.velocity.xz*=0.94f; state.spin.xyz*=0.92f;
        }
        state.bounds_max.w=float(supported);
    }
    if(length(state.angle.xyz)>0.002f) atomic_fetch_or_explicit(stats+15,1u<<id,memory_order_relaxed);
    if(state.bounds_max.w==0.0f) atomic_fetch_or_explicit(stats+16,1u<<id,memory_order_relaxed);
    else atomic_fetch_and_explicit(stats+16,~(1u<<id),memory_order_relaxed);
    if(any(state.position.xyz!=old_position) || any(state.angle.xyz!=old_angle)) {
        atomic_store_explicit(stats+17,1u,memory_order_relaxed);
        atomic_store_explicit(stats+21,1u,memory_order_relaxed);
    }
    structures(bodies,p)[id]=state;
}

kernel void structure_voxelize(RESOURCES,uint id [[thread_position_in_grid]]) {
    if(geometry_unchanged(stats)) return;
    if(id>=volume(p) || atomic_load_explicit(stats+12,memory_order_relaxed)==0u) return;
    uint value=cells[volume(p)+id];
    if(value==0u) return;
    uint3 rest=coordinate(id,p.world.x);
    uint building=structure_index(rest,p.world.x);
    if(building>=9u) return;
    Structure state=structures(bodies,p)[building];
    if(state.info.y==0u) return;
    float3 center=state.position.xyz+rotate_xyz(float3(rest)+0.5f-state.rest.xyz,state.angle.xyz);
    int3 c=int3(floor(center));
    if(inside(c,p)) atomic_fetch_max_explicit(parents+volume(p)+1u+index3(uint3(c),p.world.x),value,memory_order_relaxed);
}
