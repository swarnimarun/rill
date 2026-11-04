inline uint distance_pack(uint solid,uint air) { return min(solid,65535u)|(min(air,65535u)<<16); }
inline uint distance_relax(uint value,uint previous) {
    return distance_pack(min(value&65535u,min(65535u,(previous&65535u)+1u)),
        min(value>>16,min(65535u,(previous>>16)+1u)));
}
kernel void distance_init(RESOURCES,uint i [[thread_position_in_grid]]) {
    if(distance_unchanged(stats)||i>=volume(p)) return;
    uint3 c=coordinate(i,p.world.x);
    uint boundary=min(min(min(c.x+1u,c.y+1u),c.z+1u),min(min(p.world.x-c.x,p.world.y-c.y),p.world.z-c.z));
    field[i]=material(int3(c),p,cells,parents)!=0u ? distance_pack(0u,boundary):distance_pack(65535u,0u);
}
inline void distance_axis(uint line,uint axis,constant Params& p,device uint* field) {
    uint n=p.world.x;
    if(line>=n*n) return;
    uint3 c=uint3(0); c[(axis+1u)%3u]=line%n; c[(axis+2u)%3u]=line/n;
    uint previous=0xffffffffu;
    for(uint t=0;t<n;t++) { c[axis]=t; uint i=index3(c,n); previous=distance_relax(field[i],previous); field[i]=previous; }
    previous=0xffffffffu;
    for(uint t=n;t>0;t--) { c[axis]=t-1u; uint i=index3(c,n); previous=distance_relax(field[i],previous); field[i]=previous; }
}
kernel void distance_x(RESOURCES,uint i [[thread_position_in_grid]]) { if(!distance_unchanged(stats)) distance_axis(i,0,p,field); }
kernel void distance_y(RESOURCES,uint i [[thread_position_in_grid]]) { if(!distance_unchanged(stats)) distance_axis(i,1,p,field); }
kernel void distance_z(RESOURCES,uint i [[thread_position_in_grid]]) { if(!distance_unchanged(stats)) distance_axis(i,2,p,field); }
kernel void clear_probes(RESOURCES,uint i [[thread_position_in_grid]]) {
    if(materials_unchanged(stats)) return;
    uint n=p.limits.y; if(i<2u*n*n*n) probes[i]=float4(0);
}
inline float3 probe_position(uint3 c,constant Params& p) {
    return (float3(c)+.5f)*(float(p.world.x)/float(p.limits.y));
}
inline float3 previous_probe(float3 position,uint bank,constant Params& p,device float4* probes) {
    uint n=p.limits.y;
    uint3 c=uint3(clamp(position*float(n)/float(p.world.x),float3(0),float3(n-1u)));
    return probes[bank*n*n*n+index3(c,n)].rgb;
}
kernel void update_probes(RESOURCES,uint i [[thread_position_in_grid]]) {
    uint n=p.limits.y,count=n*n*n; if(i>=count) return;
    uint old=(p.frame.x&1u)*count,destination=((p.frame.x+1u)&1u)*count;
    float4 history=probes[old+i];
    uint stride=max(1u,p.config.w);
    if(i%stride!=p.frame.x%stride) { probes[destination+i]=history; return; }
    float3 position=probe_position(coordinate(i,n),p);
    if(material(int3(floor(position)),p,cells,parents)!=0u) { probes[destination+i]=float4(0); return; }
    uint rays=clamp(p.limits.w,4u,64u); float3 result=float3(0);
    float rotation=random01(i+p.frame.x*193u)*6.2831853f;
    for(uint ray=0;ray<rays;ray++) {
        float y=1.0f-2.0f*(float(ray)+.5f)/float(rays);
        float radius=sqrt(max(0.0f,1.0f-y*y)); float angle=float(ray)*2.3999632f+rotation;
        float3 direction=float3(cos(angle)*radius,y,sin(angle)*radius);
        Hit h=trace_indirect(position,direction,float(p.world.x)*2,p,cells,parents,bricks,field);
        if(h.status==0u) result+=sky_color(direction);
        else if(h.status==1u) {
            float3 albedo=surface_color(h.material,h.position);
            float sun=max(0.0f,dot(h.normal,sun_direction()))*visible_sun(h.position+h.normal*.025f,p,cells,parents,bricks,stats);
            float3 bounce=previous_probe(h.position+h.normal*.1f,p.frame.x&1u,p,probes)*.42f;
            result+=h.material==4u ? float3(5.0f,2.4f,.6f):albedo*(float3(1.7f,1.45f,1.1f)*sun+bounce+.018f);
        } else { atomic_fetch_add_explicit(stats+7,1u,memory_order_relaxed); }
    }
    result/=float(rays);
    probes[destination+i]=float4(mix(result,history.rgb,history.w>0 ? .78f:0.0f),1);
}
// Trilinear cache interpolation rejects probes hidden behind fresh geometry.
inline float3 gather_probes(float3 position,constant Params& p,device uint* cells,
    device atomic_uint* parents,device uint* bricks,device float4* probes,device atomic_uint* stats) {
    uint n=p.limits.y; float spacing=float(p.world.x)/float(n);
    float3 grid=position/spacing-.5f; int3 base=int3(floor(grid)); float3 f=fract(grid);
    float3 total=float3(0); float weight=0;
    uint offset=((p.frame.x+1u)&1u)*n*n*n;
    for(uint corner=0;corner<8;corner++) {
        int3 bit=int3(corner&1u,(corner>>1)&1u,(corner>>2)&1u); int3 c=base+bit;
        if(any(c<0)||any(c>=int3(n))) continue;
        float4 value=probes[offset+index3(uint3(c),n)]; if(value.w==0) continue;
        float3 target=probe_position(uint3(c),p); float3 delta=target-position; float distance=length(delta);
        if(distance>.02f) {
            uint status=trace_visibility(position,delta/distance,max(0.0f,distance-.02f),p,cells,parents,bricks);
            if(status==2u) atomic_fetch_add_explicit(stats+7,1u,memory_order_relaxed);
            if(status!=0u) continue;
        }
        float3 weights=select(1.0f-f,f,bit!=int3(0)); float w=weights.x*weights.y*weights.z;
        total+=value.rgb*w; weight+=w;
    }
    return weight>.001f ? total/weight:float3(.025f,.035f,.045f);
}
inline uint lighting_face(float3 normal) {
    return normal.x>0 ? 0u:normal.x<0 ? 1u:normal.y>0 ? 2u:normal.y<0 ? 3u:normal.z>0 ? 4u:5u;
}
inline float lighting_tag(uint face,constant Params& p) {
    // Exact finite integer encoding, with zero reserved for uninitialized data.
    return float(((p.frame.x&0xfffffu)+1u)*8u+face);
}
inline float3 image_direction(float2 sample,constant Params& p) {
    float2 uv=sample/float2(p.config.y,p.config.z)*2.0f-1.0f;
    float aspect=float(p.config.y)/float(p.config.z);
    return normalize(p.forward.xyz+p.right.xyz*uv.x*aspect*.66f-p.up.xyz*uv.y*.66f);
}
inline uint lighting_offset(constant Params& p) {
    uint n=p.limits.y; return 2u*n*n*n;
}
// Balanced GI evaluates all eight visibility-tested probes at half resolution.
// Each current-frame record carries its exact voxel and face, so interpolation
// never crosses a wall, the opposite side of a voxel, or a silhouette.
kernel void lighting_cache(RESOURCES,uint2 pixel [[thread_position_in_grid]]) {
    uint stride=p.limits.z; if(stride<=1u) return;
    uint2 size=(uint2(p.config.y,p.config.z)+stride-1u)/stride;
    if(any(pixel>=size)) return;
    uint address=lighting_offset(p)+2u*(pixel.x+size.x*pixel.y);
    float2 sample=(float2(pixel)+.5f)*float(stride);
    float3 camera=p.controller.x==0u ? p.eye.xyz:player_state(bodies,p)->eye_mode.xyz;
    Hit h=trace_voxels(camera,image_direction(sample,p),float(p.world.x)*3,p,cells,parents,bricks);
    float4 lighting=float4(0),geometry=float4(-1);
    if(h.status==1u) {
        float3 outside=h.position+h.normal*.025f;
        lighting=float4(gather_probes(outside,p,cells,parents,bricks,probes,stats),h.distance);
        geometry=float4(float3(h.cell),lighting_tag(lighting_face(h.normal),p));
    } else if(h.status==2u) atomic_fetch_add_explicit(stats+7,1u,memory_order_relaxed);
    probes[address]=lighting; probes[address+1u]=geometry;
}
inline float3 image_indirect(uint2 pixel,Hit hit,float3 outside,constant Params& p,
    device uint* cells,device atomic_uint* parents,device uint* bricks,
    device float4* probes,device atomic_uint* stats) {
    uint stride=p.limits.z;
    if(stride>1u) {
        uint2 size=(uint2(p.config.y,p.config.z)+stride-1u)/stride;
        float2 grid=(float2(pixel)+.5f)/float(stride)-.5f;
        int2 base=int2(floor(grid)); float2 f=fract(grid);
        float3 result=float3(0); float weight=0;
        uint offset=lighting_offset(p); uint face=lighting_face(hit.normal);
        for(uint corner=0;corner<4u;corner++) {
            int2 bit=int2(corner&1u,(corner>>1u)&1u),cell=base+bit;
            if(any(cell<0)||any(cell>=int2(size))) continue;
            uint address=offset+2u*(uint(cell.x)+size.x*uint(cell.y));
            float4 geometry=probes[address+1u];
            if(any(geometry.xyz!=float3(hit.cell))||geometry.w!=lighting_tag(face,p)) continue;
            float4 lighting=probes[address];
            // Points on the same voxel face are at most sqrt(2) apart. The
            // depth check also rejects malformed/stale records conservatively.
            if(abs(lighting.w-hit.distance)>1.5f) continue;
            float2 w=select(1.0f-f,f,bit!=int2(0)); float contribution=w.x*w.y;
            result+=lighting.rgb*contribution; weight+=contribution;
        }
        if(weight>.0001f) return result/weight;
    }
    // Subpixel geometry and all rejected surface boundaries retain exact GI.
    return gather_probes(outside,p,cells,parents,bricks,probes,stats);
}
kernel void render_world(RESOURCES,texture2d<float,access::write> output [[texture(0)]],uint2 pixel [[thread_position_in_grid]]) {
    uint2 size=uint2(p.config.y,p.config.z); if(any(pixel>=size)) return;
    float3 direction=image_direction(float2(pixel)+.5f,p);
    float3 camera=p.controller.x==0u ? p.eye.xyz:player_state(bodies,p)->eye_mode.xyz;
    Hit h=trace_voxels(camera,direction,float(p.world.x)*3,p,cells,parents,bricks);
    float3 color=sky_color(direction);
    if(h.status==1u) {
        float3 albedo=surface_color(h.material,h.position); float3 outside=h.position+h.normal*.025f;
        float sun=max(0.0f,dot(h.normal,sun_direction()))*visible_sun(outside,p,cells,parents,bricks,stats);
        float3 indirect=image_indirect(pixel,h,outside,p,cells,parents,bricks,probes,stats);
        // Nearby occupancy adds contact shading without stale caches.
        float occlusion=0; int3 cell=int3(floor(outside));
        occlusion+=material(cell+int3(1,0,0),p,cells,parents)!=0u ? 1.0f:0.0f;
        occlusion+=material(cell+int3(-1,0,0),p,cells,parents)!=0u ? 1.0f:0.0f;
        occlusion+=material(cell+int3(0,1,0),p,cells,parents)!=0u ? 1.0f:0.0f;
        occlusion+=material(cell+int3(0,0,1),p,cells,parents)!=0u ? 1.0f:0.0f;
        float ao=1.0f-occlusion*.105f;
        color=albedo*(float3(1.65f,1.40f,1.03f)*sun+indirect*1.35f+.035f)*ao;
        if(h.material==4u) color=float3(5,2.4,.65);
        if(p.frame.y==1u) color=albedo*(.35f+.65f*max(0.0f,dot(h.normal,sun_direction())));
        if(p.frame.y==2u) {
            uint packed=field[index3(uint3(h.cell),p.world.x)];
            float d=float(packed>>16); color=mix(float3(.16,.28,.39),float3(1,.24,.08),clamp(d/12.0f,0.0f,1.0f));
        }
        float fog=1.0f-exp(-h.distance*.0012f); color=mix(color,sky_color(direction),fog);
    } else if(h.status==2u) { color=float3(1,0,1); atomic_fetch_add_explicit(stats+7,1u,memory_order_relaxed); }
    color*=max(.01f,p.dynamics.w); color=color/(color+float3(1)); color=pow(max(color,float3(0)),float3(1.0f/2.2f));
    float2 center=abs(float2(pixel)-float2(size)*.5f);
    if((center.x<1 && center.y<7)||(center.y<1 && center.x<7)) color=float3(.95,.91,.77);
    // Discrete counters are visible as compact meters; detailed values use readback.
    if(pixel.y>size.y-12u && pixel.y<size.y-7u && pixel.x>12u && pixel.x<180u) {
        uint bodies_count=atomic_load_explicit(stats,memory_order_relaxed);
        color=pixel.x-12u<min(168u,bodies_count*168u/max(1u,p.limits.x)) ? float3(.93,.55,.20):float3(.10,.12,.13);
    }
    output.write(float4(clamp(color,0.0f,1.0f),1),pixel);
}

kernel void pick_ray(RESOURCES,uint i [[thread_position_in_grid]]) {
    if(i!=0u) return;
    float3 camera=p.controller.x==0u ? p.eye.xyz:player_state(bodies,p)->eye_mode.xyz;
    Hit h=trace_voxels(camera,normalize(p.forward.xyz),float(p.world.x)*3,p,cells,parents,bricks);
    uint valid=h.status==1u ? 1u:0u;
    float3 center=float3(h.cell)+.5f;
    atomic_store_explicit(stats+8,as_type<uint>(center.x),memory_order_relaxed);
    atomic_store_explicit(stats+9,as_type<uint>(center.y),memory_order_relaxed);
    atomic_store_explicit(stats+10,as_type<uint>(center.z),memory_order_relaxed);
    atomic_store_explicit(stats+11,valid,memory_order_relaxed);
    if(h.status==2u) atomic_fetch_add_explicit(stats+7,1u,memory_order_relaxed);
}
