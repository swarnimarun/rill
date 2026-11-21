// Units are quarter-meter voxels. This kinematic AABB controller reads the
// resident static world and dynamic debris overlay; no world readback is needed.
constant float PLAYER_RADIUS=1.2f;
constant float PLAYER_STANDING=7.2f;
constant float PLAYER_CROUCHED=6.0f;
constant float PLAYER_EYE_OFFSET=.6f;
constant float PLAYER_SKIN=.001f;

inline bool player_solid(int3 cell,constant Params& p,device uint* cells,device atomic_uint* parents) {
    if(cell.y<0||cell.x<0||cell.z<0||cell.x>=int(p.world.x)||cell.z>=int(p.world.z)) return true;
    return material(cell,p,cells,parents)!=0u;
}
inline bool player_overlap(float3 feet,float height,constant Params& p,
    device uint* cells,device atomic_uint* parents) {
    float3 low=feet-float3(PLAYER_RADIUS,0,PLAYER_RADIUS);
    float3 high=feet+float3(PLAYER_RADIUS,height,PLAYER_RADIUS);
    int3 begin=int3(floor(low+PLAYER_SKIN)),end=int3(floor(high-PLAYER_SKIN));
    for(int z=begin.z;z<=end.z;z++) for(int y=begin.y;y<=end.y;y++) for(int x=begin.x;x<=end.x;x++)
        if(player_solid(int3(x,y,z),p,cells,parents)) return true;
    return false;
}

// Sweep all voxel faces crossed along one axis, rather than checking only the
// destination. Other axes retain their exact AABB overlap, allowing wall slides.
inline float player_sweep(float3 feet,float height,uint axis,float delta,
    constant Params& p,device uint* cells,device atomic_uint* parents) {
    if(abs(delta)<1e-8f) return 0;
    float3 low=feet-float3(PLAYER_RADIUS,0,PLAYER_RADIUS);
    float3 high=feet+float3(PLAYER_RADIUS,height,PLAYER_RADIUS);
    float3 destination_low=low,destination_high=high;
    destination_low[axis]+=delta; destination_high[axis]+=delta;
    int3 begin=int3(floor(min(low,destination_low)+PLAYER_SKIN));
    int3 end=int3(floor(max(high,destination_high)-PLAYER_SKIN));
    float allowed=delta;
    for(int z=begin.z;z<=end.z;z++) for(int y=begin.y;y<=end.y;y++) for(int x=begin.x;x<=end.x;x++) {
        int3 cell=int3(x,y,z);
        bool overlap=true;
        for(uint a=0;a<3;a++) if(a!=axis)
            overlap=overlap && high[a]>float(cell[a])+PLAYER_SKIN && low[a]<float(cell[a]+1)-PLAYER_SKIN;
        if(!overlap||!player_solid(cell,p,cells,parents)) continue;
        if(delta>0 && float(cell[axis]+1)>low[axis]+PLAYER_SKIN) {
            float contact=float(cell[axis])-high[axis]-PLAYER_SKIN;
            allowed=min(allowed,max(0.0f,contact));
        }
        if(delta<0 && float(cell[axis])<high[axis]-PLAYER_SKIN) {
            float contact=float(cell[axis]+1)-low[axis]+PLAYER_SKIN;
            allowed=max(allowed,min(0.0f,contact));
        }
    }
    return allowed;
}
inline bool player_ground(float3 feet,float height,constant Params& p,
    device uint* cells,device atomic_uint* parents) {
    return player_sweep(feet,height,1u,-.08f,p,cells,parents)>-.079f;
}
inline float3 player_horizontal(float3 feet,float height,float2 movement,
    constant Params& p,device uint* cells,device atomic_uint* parents) {
    feet.x+=player_sweep(feet,height,0u,movement.x,p,cells,parents);
    feet.z+=player_sweep(feet,height,2u,movement.y,p,cells,parents);
    return feet;
}
inline float3 player_walk_horizontal(float3 feet,float height,float2 movement,bool grounded,bool sneak,
    constant Params& p,device uint* cells,device atomic_uint* parents) {
    // Sneaking refuses unsupported horizontal motion. Test each axis separately
    // so a ledge still permits motion parallel to its edge.
    if(sneak&&grounded) {
        float3 candidate=feet; candidate.x+=movement.x;
        if(!player_ground(candidate,height,p,cells,parents)) movement.x=0;
        candidate=feet; candidate.x+=movement.x; candidate.z+=movement.y;
        if(!player_ground(candidate,height,p,cells,parents)) movement.y=0;
    }
    float3 ordinary=player_horizontal(feet,height,movement,p,cells,parents);
    float2 achieved=ordinary.xz-feet.xz;
    if(!grounded||sneak||length_squared(achieved)+.00001f>=length_squared(movement)) return ordinary;
    // Step a curb only when the complete upward sweep has head clearance,
    // the horizontal sweep makes progress, and a downward sweep finds support.
    const float step_height=2.4f;
    float lift=player_sweep(feet,height,1u,step_height,p,cells,parents);
    if(lift<step_height-.002f) return ordinary;
    float3 raised=feet+float3(0,lift,0);
    raised=player_horizontal(raised,height,movement,p,cells,parents);
    float drop=player_sweep(raised,height,1u,-step_height-.09f,p,cells,parents);
    if(drop<=-step_height-.089f) return ordinary;
    raised.y+=drop;
    return length_squared(raised.xz-feet.xz)>length_squared(achieved)+.00001f ? raised:ordinary;
}

inline float3 player_bound_feet(float3 feet,constant Params& p) {
    feet.x=clamp(feet.x,PLAYER_RADIUS+PLAYER_SKIN,float(p.world.x)-PLAYER_RADIUS-PLAYER_SKIN);
    feet.z=clamp(feet.z,PLAYER_RADIUS+PLAYER_SKIN,float(p.world.z)-PLAYER_RADIUS-PLAYER_SKIN);
    feet.y=max(PLAYER_SKIN,feet.y);
    return feet;
}
inline bool player_valid(float3 feet,float height,constant Params& p,
    device uint* cells,device atomic_uint* parents) {
    return all(isfinite(feet)) && all(feet==player_bound_feet(feet,p)) &&
        !player_overlap(feet,height,p,cells,parents);
}
inline float4 player_recover(float3 original,float height,float3 saved,float3 spawn,
    constant Params& p,device uint* cells,device atomic_uint* parents) {
    float3 base=player_bound_feet(original,p);
    if(player_valid(base,height,p,cells,parents)) return float4(base,1);
    // Search at most sixteen cells, in half-cell increments. At each radius
    // upward clearance wins, followed by the four horizontal directions.
    for(uint radius=1;radius<=32;radius++) {
        float distance=float(radius)*.5f;
        for(uint direction=0;direction<5;direction++) {
            float3 offset=direction==0u ? float3(0,distance,0):
                direction==1u ? float3(distance,0,0):direction==2u ? float3(-distance,0,0):
                direction==3u ? float3(0,0,distance):float3(0,0,-distance);
            float3 candidate=base+offset;
            if(player_valid(candidate,height,p,cells,parents)) return float4(candidate,1);
        }
    }
    if(player_valid(saved,height,p,cells,parents)) return float4(saved,1);
    if(player_valid(spawn,height,p,cells,parents)) return float4(spawn,1);
    return float4(original,0);
}

kernel void player_step(RESOURCES,uint i [[thread_position_in_grid]]) {
    if(i!=0u) return;
    device Player* state=player_state(bodies,p);
    if(p.controller.y!=0u||state->velocity_height.w<1.0f) {
        state->feet_ground=float4(p.eye.xyz-float3(0,PLAYER_STANDING-PLAYER_EYE_OFFSET,0),0);
        state->velocity_height=float4(0,0,0,PLAYER_STANDING);
        state->eye_mode=float4(p.eye.xyz,float(p.controller.x));
        float3 safe=state->feet_ground.xyz;
        state->flags=uint4(0,as_type<uint>(safe.x),as_type<uint>(safe.y),as_type<uint>(safe.z));
    }
    if(p.controller.x==0u) { state->eye_mode=float4(p.eye.xyz,0); return; }
    float dt=clamp(p.dynamics.x,0.0f,.05f);
    bool active=p.controller.w!=0u;
    float4 input=active ? clamp(p.movement,-1.0f,1.0f):float4(0);
    bool jump=input.z>.5f, crouch=input.w>.5f;
    float2 stick=input.xy; if(length_squared(stick)>1.0f) stick=normalize(stick);
    float3 feet=state->feet_ground.xyz;
    float3 velocity=state->velocity_height.xyz;
    float height=state->velocity_height.w;
    if(uint(state->eye_mode.w)!=p.controller.x) velocity=float3(0);
    if(p.controller.x==2u) {
        float3 desired=p.right.xyz*stick.x+p.forward.xyz*stick.y+float3(0,input.z-input.w,0);
        if(length_squared(desired)>1.0f) desired=normalize(desired);
        feet+=desired*(p.controller.z!=0u ? 48.0f:24.0f)*dt;
        feet=clamp(feet,float3(-float(p.world.x)),float3(float(p.world.x)*2));
        state->feet_ground=float4(feet,0); state->velocity_height=float4(0,0,0,height);
        state->eye_mode=float4(feet+float3(0,height-PLAYER_EYE_OFFSET,0),2);
        state->flags.x=jump ? 1u:0u; return;
    }
    if(crouch) height=PLAYER_CROUCHED;
    else if(!player_overlap(feet,PLAYER_STANDING,p,cells,parents)) height=PLAYER_STANDING;
    else if(!player_overlap(feet,PLAYER_CROUCHED,p,cells,parents)) height=PLAYER_CROUCHED;
    if(!player_valid(feet,height,p,cells,parents)) {
        float3 saved=float3(as_type<float>(state->flags.y),as_type<float>(state->flags.z),as_type<float>(state->flags.w));
        float3 spawn=p.eye.xyz-float3(0,PLAYER_STANDING-PLAYER_EYE_OFFSET,0);
        float4 recovered=player_recover(feet,height,saved,spawn,p,cells,parents);
        if(recovered.w==0) {
            // No safe bounded recovery exists. Retain the position, stop motion,
            // and expose the failure rather than claiming a successful escape.
            atomic_fetch_add_explicit(stats+25,1u,memory_order_relaxed);
            state->feet_ground=float4(feet,0); state->velocity_height=float4(0,0,0,height);
            state->eye_mode=float4(feet+float3(0,height-PLAYER_EYE_OFFSET,0),1);
            state->flags.x=jump ? 1u:0u; return;
        }
        feet=recovered.xyz; velocity=float3(0);
        atomic_fetch_add_explicit(stats+24,1u,memory_order_relaxed);
    }
    bool grounded=velocity.y<=0 && player_ground(feet,height,p,cells,parents);
    if(jump&&state->flags.x==0u&&grounded) { velocity.y=25.0f; grounded=false; }
    float3 forward=float3(p.forward.x,0,p.forward.z);
    forward=length_squared(forward)>.0001f ? normalize(forward):float3(0,0,-1);
    float3 right=float3(forward.z*-1.0f,0,forward.x);
    float speed=height<PLAYER_STANDING-.01f ? 5.2f:(p.controller.z!=0u ? 22.4f:17.2f);
    float3 wish=(right*stick.x+forward*stick.y)*speed;
    velocity.x=wish.x; velocity.z=wish.z;
    uint substeps=max(1u,uint(ceil(dt/.0125f))); float step=dt/float(substeps);
    for(uint sub=0;sub<substeps;sub++) {
        bool was_grounded=grounded;
        velocity.y=max(-80.0f,velocity.y-64.0f*step);
        float vertical=velocity.y*step;
        float allowed=player_sweep(feet,height,1u,vertical,p,cells,parents);
        feet.y+=allowed;
        grounded=vertical<0 && allowed>vertical+.00001f;
        if(abs(allowed-vertical)>.00001f) velocity.y=0;
        feet=player_walk_horizontal(feet,height,velocity.xz*step,was_grounded||grounded,height<PLAYER_STANDING-.01f,p,cells,parents);
        if(velocity.y<=0) grounded=player_ground(feet,height,p,cells,parents);
    }
    state->feet_ground=float4(feet,grounded ? 1.0f:0.0f);
    state->velocity_height=float4(velocity,height);
    state->eye_mode=float4(feet+float3(0,height-PLAYER_EYE_OFFSET,0),1);
    state->flags.x=jump ? 1u:0u;
    if(player_valid(feet,height,p,cells,parents))
        state->flags.yzw=uint3(as_type<uint>(feet.x),as_type<uint>(feet.y),as_type<uint>(feet.z));
}
