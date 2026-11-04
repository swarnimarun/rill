// GPU-generated indirect group counts keep unchanged geometry at one guarded
// workgroup instead of launching millions of early-returning threads.
kernel void prepare_geometry(constant Params& p [[buffer(0)]],
    device atomic_uint* stats [[buffer(1)]], device uint* commands [[buffer(2)]],
    uint id [[thread_position_in_grid]]) {
    if(id!=0u) return;
    bool rebuild=atomic_load_explicit(stats+12,memory_order_relaxed)==0u ||
        atomic_load_explicit(stats+17,memory_order_relaxed)!=0u;
    if(rebuild) atomic_fetch_add_explicit(stats+19,1u,memory_order_relaxed);
    bool field=atomic_load_explicit(stats+12,memory_order_relaxed)==0u ||
        atomic_load_explicit(stats+21,memory_order_relaxed)!=0u;
    uint counts[8]={p.world.w,p.limits.x*64u,p.world.w,
        p.config.x*p.config.x*p.config.x,p.world.w,
        p.world.x*p.world.x,p.world.x*p.world.x,p.world.x*p.world.x};
    for(uint i=0;i<8;i++) {
        commands[i*3]=(i<4u?rebuild:field) ? (counts[i]+63u)/64u:1u;
        commands[i*3+1]=1u; commands[i*3+2]=1u;
    }
}

// Commit flags only in separate passes: clearing a guard inside a parallel
// producer would let not-yet-scheduled workgroups incorrectly skip their work.
kernel void geometry_complete(RESOURCES,uint id [[thread_position_in_grid]]) {
    if(id==0u) atomic_store_explicit(stats+17,0u,memory_order_relaxed);
}
kernel void distance_complete(RESOURCES,uint id [[thread_position_in_grid]]) {
    if(id==0u) {
        if(!distance_unchanged(stats)) atomic_fetch_add_explicit(stats+26,1u,memory_order_relaxed);
        atomic_store_explicit(stats+21,0u,memory_order_relaxed);
    }
}
