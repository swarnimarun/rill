// Included by gpu.c so fixtures exercise the actual resource/dispatch path.
static bool interaction_failure(const char *message) {
    fprintf(stderr,"GPU interaction selftest: %s\n",message);
    return false;
}
static uint32_t interaction_player_offset(void) {
    return BODY_CAPACITY*80u+9u*(uint32_t)sizeof(GPUStructure);
}
static bool interaction_player_write(const GPUPlayer *state) {
    return begin_command() && upload(4,state,sizeof *state,interaction_player_offset()) && finish(true);
}
static GPUPlayer interaction_player(float x,float y,float z,float height) {
    GPUPlayer state={0};
    state.feet_ground[0]=x; state.feet_ground[1]=y; state.feet_ground[2]=z; state.feet_ground[3]=1;
    state.velocity_height[3]=height;
    state.eye_mode[0]=x; state.eye_mode[1]=y+height-.6f; state.eye_mode[2]=z; state.eye_mode[3]=1;
    return state;
}
static bool interaction_player_dispatch(GPUPlayer *state) {
    return begin_command() && vg_dispatch(21) && finish(true) &&
        download(4,state,sizeof *state,interaction_player_offset());
}
static bool interaction_player_tick(GPUPlayer *state,float elapsed,float strafe,float forward,float jump,float crouch) {
    params.dynamics[0]=elapsed;
    params.movement[0]=strafe; params.movement[1]=forward;
    params.movement[2]=jump; params.movement[3]=crouch;
    params.controller[0]=1; params.controller[1]=0; params.controller[2]=0; params.controller[3]=1;
    memset(params.right,0,sizeof params.right); params.right[0]=1;
    memset(params.forward,0,sizeof params.forward); params.forward[2]=-1;
    return interaction_player_dispatch(state);
}
static void interaction_floor(uint32_t *cells) {
    memset(cells,0,params.world[3]*sizeof *cells);
    for(uint32_t z=0;z<dimension;z++) for(uint32_t x=0;x<dimension;x++) cells[cell_at(x,0,z)]=1;
}
static bool test_player_movement(uint32_t *cells) {
    interaction_floor(cells);
    GPUPlayer state=interaction_player(10,1,30,7.2f);
    if(!fixture(cells)||!interaction_player_write(&state)||!interaction_player_tick(&state,.05f,1,0,0,0)) return false;
    float cardinal=state.feet_ground[0]-10;
    state=interaction_player(10,1,30,7.2f);
    if(!interaction_player_write(&state)||!interaction_player_tick(&state,.05f,1,1,0,0)) return false;
    float dx=state.feet_ground[0]-10,dz=state.feet_ground[2]-30;
    float diagonal=sqrtf(dx*dx+dz*dz);
    if(!(cardinal>.5f && dx>0 && dz<0 && fabsf(diagonal-cardinal)<.005f))
        return interaction_failure("diagonal walking changed total horizontal speed");
    state=interaction_player(10,1,30,7.2f);
    if(!interaction_player_write(&state)) return false;
    params.movement[0]=1; params.movement[1]=0; params.controller[2]=1;
    if(!interaction_player_dispatch(&state)) return false;
    float sprint=state.feet_ground[0]-10;
    if(!(sprint>cardinal*1.2f && sprint<cardinal*2))
        return interaction_failure("sprint failed to increase grounded walking speed");

    interaction_floor(cells);
    for(unsigned z=8;z<=12;z++) for(unsigned x=14;x<=17;x++) for(unsigned y=1;y<=2;y++)
        cells[cell_at(x,y,z)]=2;
    state=interaction_player(11,1,10,7.2f);
    if(!fixture(cells)||!interaction_player_write(&state)) return false;
    for(unsigned tick=0;tick<6;tick++) if(!interaction_player_tick(&state,.05f,1,0,0,0)) return false;
    if(!(state.feet_ground[0]>15.5f && fabsf(state.feet_ground[1]-3)<.02f && state.feet_ground[3]>0))
        return interaction_failure("grounded player failed to step onto a two-voxel curb");
    for(unsigned z=8;z<=12;z++) for(unsigned x=14;x<=17;x++) cells[cell_at(x,3,z)]=2;
    state=interaction_player(11,1,10,7.2f);
    if(!fixture(cells)||!interaction_player_write(&state)) return false;
    for(unsigned tick=0;tick<8;tick++) if(!interaction_player_tick(&state,.05f,1,0,0,0)) return false;
    if(!(state.feet_ground[0]<=12.801f && fabsf(state.feet_ground[1]-1)<.02f))
        return interaction_failure("automatic stepping climbed a three-voxel wall");

    interaction_floor(cells);
    for(unsigned z=8;z<=20;z++) for(unsigned x=8;x<=14;x++) for(unsigned y=1;y<=8;y++)
        cells[cell_at(x,y,z)]=2;
    state=interaction_player(13,9,12,7.2f);
    if(!fixture(cells)||!interaction_player_write(&state)) return false;
    for(unsigned tick=0;tick<16;tick++) if(!interaction_player_tick(&state,.05f,1,0,0,1)) return false;
    if(!(state.feet_ground[0]>14.5f && state.feet_ground[0]<=16.201f &&
         fabsf(state.feet_ground[1]-9)<.02f && state.feet_ground[3]>0))
        return interaction_failure("crouching failed to stop unsupported movement over a ledge");
    for(unsigned tick=0;tick<10;tick++) if(!interaction_player_tick(&state,.05f,1,1,0,1)) return false;
    if(!(state.feet_ground[0]<=16.201f && state.feet_ground[2]<11 &&
         fabsf(state.feet_ground[1]-9)<.02f && state.feet_ground[3]>0))
        return interaction_failure("crouching at a ledge blocked parallel motion or allowed falling");
    for(unsigned tick=0;tick<10;tick++) if(!interaction_player_tick(&state,.05f,1,0,0,0)) return false;
    if(!(state.feet_ground[0]>17 && state.feet_ground[1]<8.8f))
        return interaction_failure("standing movement failed to walk off the same ledge");
    fprintf(stderr,"GPU selftest: curb height limit, crouch ledge safety and normalized sprint movement pass\n");
    return true;
}

static bool test_player_interaction(uint32_t *cells,uint32_t *actual) {
    interaction_floor(cells);
    for(unsigned z=4;z<=28;z++) for(unsigned y=1;y<=12;y++) cells[cell_at(20,y,z)]=2;
    GPUPlayer state=interaction_player(17,1,20,7.2f);
    if(!fixture(cells)||!interaction_player_write(&state)) return false;
    for(unsigned tick=0;tick<8;tick++) if(!interaction_player_tick(&state,.05f,1,1,0,0)) return false;
    if(!(state.feet_ground[0]>17.5f && state.feet_ground[0]<=18.801f && state.feet_ground[2]<17 &&
         fabsf(state.feet_ground[1]-1)<.02f && state.feet_ground[3]>0))
        return interaction_failure("grounded diagonal movement must slide along the wall without crossing it");

    interaction_floor(cells); state=interaction_player(10,1,10,7.2f);
    if(!fixture(cells)||!interaction_player_write(&state)||!interaction_player_tick(&state,.016f,0,0,1,0)) return false;
    if(!(state.velocity_height[1]>0 && state.feet_ground[1]>1 && state.feet_ground[3]==0))
        return interaction_failure("grounded jump did not rise");
    float velocity=state.velocity_height[1];
    for(unsigned tick=0;tick<3;tick++) {
        if(!interaction_player_tick(&state,.016f,0,0,1,0)) return false;
        if(!(state.velocity_height[1]<velocity)) return interaction_failure("holding jump retriggered an airborne jump");
        velocity=state.velocity_height[1];
    }
    if(!interaction_player_tick(&state,.016f,0,0,0,0)) return false;
    velocity=state.velocity_height[1];
    if(!interaction_player_tick(&state,.016f,0,0,1,0)) return false;
    if(!(state.velocity_height[1]<velocity)) return interaction_failure("released and pressed jump retriggered in air");
    for(unsigned tick=0;tick<90;tick++) if(!interaction_player_tick(&state,.016f,0,0,1,0)) return false;
    if(!(fabsf(state.feet_ground[1]-1)<.02f && state.feet_ground[3]>0))
        return interaction_failure("jump did not land or held jump bounced automatically");
    if(!interaction_player_tick(&state,.016f,0,0,0,0)||!interaction_player_tick(&state,.016f,0,0,1,0)) return false;
    if(!(state.velocity_height[1]>0)) return interaction_failure("jump failed after landing and releasing the button");

    interaction_floor(cells);
    for(unsigned z=8;z<=12;z++) for(unsigned x=8;x<=12;x++) cells[cell_at(x,8,z)]=2;
    state=interaction_player(10,1,10,6);
    if(!fixture(cells)||!interaction_player_write(&state)||!interaction_player_tick(&state,.016f,0,0,0,0)) return false;
    if(!(state.velocity_height[3]<=6.01f && state.feet_ground[1]+state.velocity_height[3]<8))
        return interaction_failure("crouched player stood through a low ceiling");
    for(unsigned tick=0;tick<30;tick++) if(!interaction_player_tick(&state,.05f,1,0,0,0)) return false;
    if(!(state.feet_ground[0]>14.2f && state.velocity_height[3]>7.1f))
        return interaction_failure("player did not stand after clearing the ceiling");

    // A voxelized dynamic fragment must block the same player sweep as static material.
    interaction_floor(cells); memset(actual,0,params.world[3]*sizeof *actual);
    for(unsigned z=8;z<=12;z++) for(unsigned y=1;y<=12;y++) actual[cell_at(14,y,z)]=2;
    state=interaction_player(10,1,10,7.2f);
    if(!fixture(cells)||!begin_command()||!upload(1,actual,params.world[3]*4,(params.world[3]+1)*4)||
       !finish(true)||!interaction_player_write(&state)) return false;
    for(unsigned tick=0;tick<12;tick++) if(!interaction_player_tick(&state,.05f,1,0,0,0)) return false;
    if(!(state.feet_ground[0]>12 && state.feet_ground[0]<=12.801f))
        return interaction_failure("player crossed voxelized dynamic debris");

    interaction_floor(cells);
    for(unsigned z=4;z<=28;z++) for(unsigned y=1;y<=12;y++) cells[cell_at(20,y,z)]=2;
    state=interaction_player(17,1,20,7.2f);
    if(!fixture(cells)||!interaction_player_write(&state)||!interaction_player_tick(&state,5,1,0,0,0)) return false;
    if(!(state.feet_ground[0]>17 && state.feet_ground[0]<18 && fabsf(state.feet_ground[1]-1)<.02f))
        return interaction_failure("long frame was not bounded before controller movement");
    state=interaction_player(10,20,10,7.2f); state.feet_ground[3]=0; state.velocity_height[1]=-80;
    if(!interaction_player_write(&state)) return false;
    for(unsigned tick=0;tick<12;tick++) if(!interaction_player_tick(&state,.05f,0,0,0,0)) return false;
    if(!(fabsf(state.feet_ground[1]-1)<.02f && state.feet_ground[3]>0))
        return interaction_failure("fast falling player tunneled through the floor");
    // Switching back from fly mode must retain valid placement and recover
    // invalid placement instead of leaving the player trapped in a wall.
    state=interaction_player(12,1,20,7.2f); state.eye_mode[3]=2;
    if(!interaction_player_write(&state)||!interaction_player_tick(&state,.016f,0,0,0,0)) return false;
    if(!(fabsf(state.feet_ground[0]-12)<.01f && fabsf(state.feet_ground[2]-20)<.01f))
        return interaction_failure("valid fly-to-walk transition teleported the player");
    state=interaction_player(20.5f,1,20,7.2f); state.eye_mode[3]=2;
    if(!interaction_player_write(&state)||!interaction_player_tick(&state,.016f,0,0,0,0)) return false;
    bool clear_wall=state.feet_ground[0]<=18.801f || state.feet_ground[0]>=22.199f ||
        state.feet_ground[1]>=12.999f || state.feet_ground[2]<=2.801f || state.feet_ground[2]>=30.199f;
    if(!clear_wall || !isfinite(state.feet_ground[0]) || !isfinite(state.feet_ground[1]) || !isfinite(state.feet_ground[2]))
        return interaction_failure("fly-to-walk transition left the player inside solid geometry");
    state=interaction_player(-8,1,10,7.2f); state.eye_mode[3]=2;
    if(!interaction_player_write(&state)||!interaction_player_tick(&state,.016f,0,0,0,0)) return false;
    if(!(state.feet_ground[0]>=1.199f && state.feet_ground[0]<=(float)dimension-1.199f && state.feet_ground[1]>=.999f))
        return interaction_failure("fly-to-walk transition did not recover inside world bounds");
    if(!test_player_movement(cells)) return false;
    memset(params.movement,0,sizeof params.movement); memset(params.controller,0,sizeof params.controller);
    fprintf(stderr,"GPU selftest: grounded wall slide, jump edges, crouch clearance and hitch collision pass\n");
    return true;
}

static bool interaction_structure_read(GPUStructure structures[9]) {
    return download(4,structures,9*sizeof *structures,BODY_CAPACITY*80u);
}
static bool test_structure_interaction(uint32_t *cells,uint32_t *actual) {
    uint32_t count=params.world[3];
    uint32_t *rest=malloc((size_t)count*sizeof *rest);
    if(!rest) return false;
    bool ok=false;
    GPUStructure states[9],undamaged[9],initial;
    uint64_t initial_mass=0,initial_material=0,removed=0,removed_material=0;
    if(!begin_command()||!vg_dispatch(0)||!vg_dispatch(17)||!vg_dispatch(18)||!finish(true)||
       !download(0,cells,count*4,0)||!download(0,rest,count*4,count*4)||!interaction_structure_read(states)) goto done;
    for(uint32_t i=0;i<count;i++) {
        initial_mass+=(cells[i]!=0)+(rest[i]!=0);
        initial_material+=cells[i]+rest[i];
    }
    memcpy(undamaged,states,sizeof undamaged);
    initial=states[0];
    if(initial.info[0]==0 || initial.bounds_max[3]<2) {
        interaction_failure("generated structure lacks independently destructible supporting columns"); goto done;
    }
    params.dynamics[0]=1.0f/60; params.dynamics[1]=24;
    for(unsigned tick=0;tick<60;tick++) if(!begin_command()||!vg_dispatch(19)||!finish(true)) goto done;
    if(!interaction_structure_read(states)) goto done;
    for(unsigned building=0;building<9;building++) {
        if(fabsf(states[building].angle[0])+fabsf(states[building].angle[2])>.0001f ||
           fabsf(states[building].position[1]-undamaged[building].position[1])>.005f) {
            fprintf(stderr,"intact structure %u tilt=(%g,%g) y=%g initial_y=%g anchors=%g\n",
                building,states[building].angle[0],states[building].angle[2],states[building].position[1],
                undamaged[building].position[1],states[building].bounds_max[3]);
            interaction_failure("intact structure drifted before any support was removed"); goto done;
        }
    }
    unsigned tile=dimension/3,story=dimension/12;
    if(story<5) story=5;
    // Remove only one side of this building's foundations. The other complete
    // columns still connect the assembly to the bedrock plane.
    for(unsigned z=0;z<tile;z++) for(unsigned y=1;y<story;y++) for(unsigned x=0;x<tile/2;x++) {
        uint32_t index=cell_at(x,y,z);
        if(cells[index]>1) { ++removed; removed_material+=cells[index]; cells[index]=0; }
    }
    if(!removed||!begin_command()||!upload(0,cells,count*4,0)||!vg_dispatch(18)||!finish(true)||!interaction_structure_read(states)) goto done;
    if(!(states[0].bounds_max[3]>0 && states[0].bounds_max[3]<initial.bounds_max[3] && states[0].info[0]==initial.info[0])) {
        interaction_failure("partial foundation edit must retain some supports and all upper structure mass"); goto done;
    }
    params.dynamics[0]=1.0f/60; params.dynamics[1]=24;
    bool leaning=false;
    for(unsigned tick=0;tick<60;tick++) {
        if(!begin_command()||!vg_dispatch(19)||!finish(true)||!interaction_structure_read(states)) goto done;
        float tilt=fabsf(states[0].angle[0])+fabsf(states[0].angle[2]);
        if(!isfinite(tilt)||states[0].info[0]!=initial.info[0]) {
            interaction_failure("damaged spring structure lost mass or finite orientation"); goto done;
        }
        leaning=tilt>.001f && states[0].bounds_max[3]>0;
    }
    if(!leaning) { interaction_failure("asymmetric foundation damage did not lean while support remained"); goto done; }
    GPUStructure leaned=states[0];
    for(unsigned z=0;z<tile;z++) for(unsigned y=1;y<story;y++) for(unsigned x=0;x<tile;x++) {
        uint32_t index=cell_at(x,y,z);
        if(cells[index]>1) { ++removed; removed_material+=cells[index]; cells[index]=0; }
    }
    if(!begin_command()||!upload(0,cells,count*4,0)||!vg_dispatch(18)||!finish(true)||!interaction_structure_read(states)) goto done;
    if(states[0].bounds_max[3]!=0) { interaction_failure("fully severed structure retained foundation support"); goto done; }
    for(unsigned tick=0;tick<20;tick++) if(!begin_command()||!vg_dispatch(19)||!finish(true)) goto done;
    if(!interaction_structure_read(states)||!begin_command()||!vg_dispatch(6)||!vg_dispatch(20)||!finish(true)||
       !download(0,actual,count*4,0)||!download(0,rest,count*4,count*4)) goto done;
    if(!(states[0].position[1]<leaned.position[1]-.05f && states[0].info[0]==initial.info[0])) {
        interaction_failure("fully severed structure failed to fall with its original material mass"); goto done;
    }
    uint64_t remaining_mass=0,remaining_material=0,assembly_mass=0,atlas_mass=0;
    for(uint32_t i=0;i<count;i++) {
        remaining_mass+=(actual[i]!=0)+(rest[i]!=0);
        remaining_material+=actual[i]+rest[i]; atlas_mass+=rest[i]!=0;
    }
    for(unsigned i=0;i<9;i++) assembly_mass+=states[i].info[0];
    if(remaining_mass+removed!=initial_mass || remaining_material+removed_material!=initial_material || assembly_mass!=atlas_mass) {
        interaction_failure("spring motion or voxelization changed conserved material"); goto done;
    }
    fprintf(stderr,"GPU selftest: partial-support spring lean, complete severing and material conservation pass\n");
    ok=true;
done:
    free(rest);
    return ok;
}

static bool test_input_interaction(void) {
    VoxelInput input={.focused=true};
    vi_begin(&input);
    vi_motion(&input,400,-300);
    if(input.captured || input.motion[0] || input.motion[1])
        return interaction_failure("startup must leave the cursor free and ignore relative motion");
    if(vi_click(&input,true,true)!=CAPTURE_ENABLE || !input.captured || input.fire_pressed || input.fire_down)
        return interaction_failure("first click must capture without destroying material");
    vi_motion(&input,900,900);
    vi_click(&input,true,false); vi_click(&input,true,true);
    if(input.motion[0] || input.motion[1] || input.fire_pressed || input.fire_down)
        return interaction_failure("capture-frame mouse deltas and clicks must be discarded");
    vi_begin(&input);
    vi_click(&input,true,false); vi_click(&input,true,true);
    vi_motion(&input,17,-9);
    if(!input.fire_pressed || !input.fire_down || input.motion[0]!=17 || input.motion[1]!=-9)
        return interaction_failure("captured input failed to deliver look and destruction");
    input.jump_pressed=true;
    if(vi_escape(&input,true)!=CAPTURE_DISABLE || input.captured || input.quit || input.fire_down ||
       input.fire_pressed || input.jump_pressed || input.motion[0] || input.motion[1])
        return interaction_failure("Escape must release capture and clear pending gameplay input");
    vi_begin(&input);
    if(vi_click(&input,false,true)!=CAPTURE_ENABLE) return interaction_failure("right click failed to recapture");
    vi_begin(&input); vi_motion(&input,22,33); input.fire_down=true; input.jump_pressed=true;
    if(vi_focus(&input,false)!=CAPTURE_DISABLE || input.focused || input.captured || input.fire_down ||
       input.jump_pressed || input.motion[0] || input.motion[1])
        return interaction_failure("focus loss failed to suspend captured input");
    if(vi_click(&input,true,true)!=CAPTURE_NONE || input.captured)
        return interaction_failure("unfocused mouse click captured the cursor");
    if(vi_focus(&input,true)!=CAPTURE_NONE || input.captured)
        return interaction_failure("focus gain automatically recaptured the cursor");
    vi_motion(&input,700,700);
    if(input.motion[0] || input.motion[1]) return interaction_failure("stale motion survived focus gain");
    if(vi_escape(&input,false)!=CAPTURE_NONE || input.quit)
        return interaction_failure("release-only Escape unexpectedly quit");
    if(vi_escape(&input,true)!=CAPTURE_QUIT || !input.quit)
        return interaction_failure("Escape with a free cursor failed to request quit");
    fprintf(stderr,"GPU selftest: first-click capture, Escape release and focus-safe mouse input pass\n");
    return true;
}

static bool interaction_cursor_key(SDL_Scancode code) {
    SDL_Event event={0}; event.type=SDL_EVENT_KEY_DOWN;
    event.key.windowID=SDL_GetWindowID(window); event.key.scancode=code;
    return SDL_PushEvent(&event);
}
static bool interaction_cursor_focus(Uint32 type) {
    SDL_Event event={0}; event.type=type; event.window.windowID=SDL_GetWindowID(window);
    return SDL_PushEvent(&event);
}
static bool interaction_cursor_click(void) {
    SDL_Event event={0}; event.type=SDL_EVENT_MOUSE_BUTTON_DOWN;
    event.button.windowID=SDL_GetWindowID(window); event.button.button=SDL_BUTTON_LEFT;
    if(!SDL_PushEvent(&event)) return false;
    event.type=SDL_EVENT_MOUSE_BUTTON_UP;
    return SDL_PushEvent(&event);
}
static bool interaction_cursor_move(float x,float y) {
    SDL_Event event={0}; event.type=SDL_EVENT_MOUSE_MOTION;
    event.motion.windowID=SDL_GetWindowID(window); event.motion.xrel=x; event.motion.yrel=y;
    return SDL_PushEvent(&event);
}

// This entry uses a real SDL window and checks the OS relative-mode state,
// rather than only checking the pure event reducer above.
int32_t vg_input_test(void) {
    if(!vg_open()) { int32_t result=preflight_failed?77:1; vg_close(); return result; }
    int32_t result=1;
#define INTERACTION_CURSOR_CHECK(condition) do { if(!(condition)) { \
    fprintf(stderr,"SDL cursor selftest failed line %d: %s (%s)\n",__LINE__,#condition,SDL_GetError()); \
    goto done; } } while(0)
    INTERACTION_CURSOR_CHECK(window!=NULL);
    INTERACTION_CURSOR_CHECK(vg_poll());
    INTERACTION_CURSOR_CHECK(!input.captured && !SDL_GetWindowRelativeMouseMode(window));
    INTERACTION_CURSOR_CHECK(interaction_cursor_focus(SDL_EVENT_WINDOW_FOCUS_GAINED));
    INTERACTION_CURSOR_CHECK(interaction_cursor_click() && interaction_cursor_move(999,999));
    INTERACTION_CURSOR_CHECK(vg_poll());
    INTERACTION_CURSOR_CHECK(input.captured && SDL_GetWindowRelativeMouseMode(window));
    INTERACTION_CURSOR_CHECK(!keys[7] && vg_mouse(0)==0 && vg_mouse(1)==0);
    INTERACTION_CURSOR_CHECK(interaction_cursor_click() && interaction_cursor_move(23,-17));
    INTERACTION_CURSOR_CHECK(vg_poll());
    INTERACTION_CURSOR_CHECK(keys[7] && vg_mouse(0)==23 && vg_mouse(1)==-17);
    INTERACTION_CURSOR_CHECK(interaction_cursor_key(SDL_SCANCODE_X));
    INTERACTION_CURSOR_CHECK(interaction_cursor_focus(SDL_EVENT_WINDOW_FOCUS_LOST));
    INTERACTION_CURSOR_CHECK(interaction_cursor_move(400,400) && vg_poll());
    INTERACTION_CURSOR_CHECK(!input.captured && !keys[9] && !SDL_GetWindowRelativeMouseMode(window));
    INTERACTION_CURSOR_CHECK(vg_mouse(0)==0 && vg_mouse(1)==0);
    INTERACTION_CURSOR_CHECK(interaction_cursor_focus(SDL_EVENT_WINDOW_FOCUS_GAINED) && vg_poll());
    INTERACTION_CURSOR_CHECK(!input.captured && !SDL_GetWindowRelativeMouseMode(window));
    INTERACTION_CURSOR_CHECK(interaction_cursor_click() && vg_poll() && input.captured);
    INTERACTION_CURSOR_CHECK(interaction_cursor_key(SDL_SCANCODE_ESCAPE) && vg_poll());
    INTERACTION_CURSOR_CHECK(!input.captured && !input.quit && !SDL_GetWindowRelativeMouseMode(window));
    INTERACTION_CURSOR_CHECK(interaction_cursor_key(SDL_SCANCODE_TAB) && vg_poll() && !input.quit);
    INTERACTION_CURSOR_CHECK(interaction_cursor_key(SDL_SCANCODE_ESCAPE));
    INTERACTION_CURSOR_CHECK(!vg_poll() && input.quit);
    fprintf(stderr,"SDL selftest: native cursor capture/release and queued focus input pass\n");
    result=0;
done:
    vg_close();
#undef INTERACTION_CURSOR_CHECK
    return result;
}

static bool interaction_geometry_counts(uint32_t commands[24]) {
    if(!begin_command()) return false;
    SDL_GPUTransferBufferCreateInfo info={.usage=SDL_GPU_TRANSFERBUFFERUSAGE_DOWNLOAD,.size=24*4};
    SDL_GPUTransferBuffer *transfer=SDL_CreateGPUTransferBuffer(device,&info);
    if(!transfer) return false;
    SDL_GPUCopyPass *copy=SDL_BeginGPUCopyPass(command);
    SDL_GPUBufferRegion source={.buffer=geometry_dispatch,.size=24*4};
    SDL_GPUTransferBufferLocation destination={.transfer_buffer=transfer};
    SDL_DownloadFromGPUBuffer(copy,&source,&destination); SDL_EndGPUCopyPass(copy);
    bool ok=finish(true);
    if(ok) {
        void *mapped=SDL_MapGPUTransferBuffer(device,transfer,false);
        if(mapped) { memcpy(commands,mapped,24*4); SDL_UnmapGPUTransferBuffer(device,transfer); }
        else ok=false;
    }
    SDL_ReleaseGPUTransferBuffer(device,transfer);
    return ok;
}
static bool interaction_geometry_dispatches(bool geometry, bool distances) {
    uint32_t commands[24];
    if(!interaction_geometry_counts(commands)) return false;
    uint32_t volume_groups=(params.world[3]+63)/64;
    uint32_t rows=(dimension*dimension+63)/64;
    uint32_t brick_groups=(params.config[0]*params.config[0]*params.config[0]+63)/64;
    uint32_t expected[8]={volume_groups,params.limits[0],volume_groups,brick_groups,volume_groups,rows,rows,rows};
    for(unsigned slot=0;slot<8;slot++) {
        bool rebuild=slot<4?geometry:distances;
        uint32_t x=rebuild?expected[slot]:1;
        if(commands[slot*3]!=x || commands[slot*3+1]!=1 || commands[slot*3+2]!=1)
            return interaction_failure("indirect geometry commands disagree with cache validity and buffer extents");
    }
    return true;
}
static bool interaction_reconstruct(void) {
    return vg_dispatch(23)&&vg_dispatch(6)&&vg_dispatch(7)&&vg_dispatch(20)&&vg_dispatch(8)&&vg_dispatch(25)&&
        vg_dispatch(9)&&vg_dispatch(10)&&vg_dispatch(11)&&vg_dispatch(12)&&vg_dispatch(26);
}
static bool test_geometry_cache(uint32_t *cells,uint32_t *actual) {
    uint32_t bytes=params.world[3]*4,overlay_offset=(params.world[3]+1)*4;
    uint32_t *field=malloc(bytes),*overlay=malloc(bytes);
    uint32_t *parents=malloc((size_t)bytes+4),*after_parents=malloc((size_t)bytes+4);
    bool ok=false;
    uint32_t counters[64],rebuilds;
    if(!field||!overlay||!parents||!after_parents) goto done;
    params.dynamics[0]=1.0f/60; params.dynamics[1]=24;
    // Geometry may be rebuilt for collision before the SDF is needed. Its
    // completion must clear only geometry dirtiness, retaining pending fields.
    if(!begin_command()||!vg_dispatch(0)||!vg_dispatch(17)||!vg_dispatch(18)||!vg_dispatch(23)||
       !vg_dispatch(6)||!vg_dispatch(7)||!vg_dispatch(20)||!vg_dispatch(8)||!vg_dispatch(25)||
       !finish(true)||!download(7,counters,sizeof counters,0)) goto done;
    if(counters[17]!=0||counters[18]!=1||counters[21]!=1||counters[19]!=1||counters[26]!=0) {
        interaction_failure("geometry completion lost pending distance work or retained geometry dirtiness"); goto done;
    }
    if(!interaction_geometry_dispatches(true,true)) goto done;
    // A new frame clears per-frame edit flags, never unfinished distance work.
    if(!begin_command()||!vg_dispatch(22)||!vg_dispatch(23)||!finish(true)||
       !download(7,counters,sizeof counters,0)) goto done;
    if(counters[17]!=0||counters[18]!=0||counters[21]!=1||counters[19]!=1||counters[26]!=0) {
        interaction_failure("frame begin dropped pending fields or duplicated geometry work"); goto done;
    }
    if(!interaction_geometry_dispatches(false,true)||!begin_command()||!vg_dispatch(23)||
       !vg_dispatch(9)||!vg_dispatch(10)||!vg_dispatch(11)||!vg_dispatch(12)||!vg_dispatch(26)||!finish(true)||
       !download(7,counters,sizeof counters,0)) goto done;
    if(counters[17]!=0||counters[18]!=0||counters[21]!=0||counters[19]!=1||counters[26]!=1) {
        interaction_failure("distance completion did not consume only the pending field work"); goto done;
    }
    rebuilds=counters[19];
    if(!download(2,field,bytes,0)||!download(1,overlay,bytes,overlay_offset)) goto done;
    if(!begin_command()||!vg_dispatch(22)||!vg_dispatch(19)||!vg_dispatch(5)||
       !interaction_reconstruct()||!finish(true)||!download(7,counters,sizeof counters,0)) goto done;
    if(counters[17]!=0||counters[18]!=0||counters[21]!=0||counters[19]!=rebuilds||counters[26]!=1) {
        interaction_failure("intact simulation invalidated retained geometry"); goto done;
    }
    if(!interaction_geometry_dispatches(false,false)||!download(2,actual,bytes,0)) goto done;
    if(memcmp(actual,field,bytes)!=0) { interaction_failure("retained distance field changed on an intact frame"); goto done; }
    if(!download(1,actual,bytes,overlay_offset)||memcmp(actual,overlay,bytes)!=0) {
        interaction_failure("retained occupancy changed on an intact frame"); goto done;
    }
    if(!download(1,parents,bytes+4,0)) goto done;
    params.frame[3]=0; params.edit[0]=params.edit[1]=params.edit[2]=(float)dimension+100; params.edit[3]=1;
    if(!begin_command()||!vg_dispatch(22)||!vg_dispatch(1)||!vg_dispatch(18)||!vg_dispatch(2)||
       !vg_dispatch(3)||!vg_dispatch(4)||!interaction_reconstruct()||!finish(true)||
       !download(7,counters,sizeof counters,0)||!download(1,after_parents,bytes+4,0)) goto done;
    if(counters[17]!=0||counters[18]!=0||counters[21]!=0||counters[19]!=rebuilds||counters[26]!=1||memcmp(parents,after_parents,bytes+4)!=0) {
        interaction_failure("missed edit changed support or invalidated retained geometry"); goto done;
    }
    if(!interaction_geometry_dispatches(false,false)||!download(2,actual,bytes,0)) goto done;
    if(memcmp(actual,field,bytes)!=0) { interaction_failure("missed edit changed retained distances"); goto done; }
    unsigned center=dimension/6;
    params.edit[0]=params.edit[2]=(float)center+.5f; params.edit[1]=1.5f;
    if(!begin_command()||!vg_dispatch(22)||!vg_dispatch(1)||!vg_dispatch(18)||!vg_dispatch(2)||
       !vg_dispatch(3)||!vg_dispatch(4)||!finish(true)||!download(7,counters,sizeof counters,0)) goto done;
    if(counters[17]!=1||counters[18]!=1||counters[21]!=1||counters[1]==0) {
        interaction_failure("real edit did not invalidate geometry and distances independently"); goto done;
    }
    if(!begin_command()||!interaction_reconstruct()||!finish(true)||
       !download(7,counters,sizeof counters,0)||!download(2,actual,bytes,0)) goto done;
    if(counters[17]!=0||counters[18]!=1||counters[21]!=0||counters[19]!=rebuilds+1||counters[26]!=2||counters[1]==0||counters[3]!=0||
       memcmp(actual,field,bytes)==0) {
        interaction_failure("real foundation destruction failed to rebuild dirty geometry"); goto done;
    }
    if(!interaction_geometry_dispatches(true,true)||!download(0,actual,bytes,0)) goto done;
    if(actual[cell_at(center,1,center)]!=0) { interaction_failure("foundation edit fixture missed its intended material"); goto done; }
    // Cutting only a rigid building's rest atlas must refresh its mass and GI
    // without rebuilding static support roots.
    GPUStructure before_structures[9],after_structures[9];
    float marker[4]={1,2,3,4},probe[4];
    unsigned story=dimension/12; if(story<5) story=5;
    if(!interaction_structure_read(before_structures)||!download(1,parents,bytes+4,0)||
       !download(2,field,bytes,0)) goto done;
    // This valid unused air-node root would be reset by support_seed. It is
    // unreachable from occupied cells and does not alter support semantics.
    parents[params.world[3]]=0;
    params.edit[0]=params.edit[2]=(float)center+.5f; params.edit[1]=(float)story+.5f; params.edit[3]=.49f;
    if(!begin_command()||!vg_dispatch(22)||!upload(1,&parents[params.world[3]],4,bytes)||
       !upload(6,marker,sizeof marker,0)||!vg_dispatch(1)||
       !vg_dispatch(18)||!vg_dispatch(2)||!vg_dispatch(3)||!vg_dispatch(4)||!vg_dispatch(15)||!finish(true)||
       !download(7,counters,sizeof counters,0)||!download(1,after_parents,bytes+4,0)||
       !interaction_structure_read(after_structures)||!download(6,probe,sizeof probe,0)) goto done;
    if(counters[18]!=2||counters[17]!=1||counters[21]!=1||
       after_structures[0].info[0]+1!=before_structures[0].info[0]||
       memcmp(parents,after_parents,bytes+4)!=0||probe[0]!=0||probe[1]!=0||probe[2]!=0||probe[3]!=0) {
        interaction_failure("rest-only cut failed to update mass/GI or unexpectedly changed static support"); goto done;
    }
    rebuilds=counters[19];
    if(!begin_command()||!interaction_reconstruct()||!finish(true)||!download(2,actual,bytes,0)||
       !download(7,counters,sizeof counters,0)) goto done;
    if(counters[17]!=0||counters[21]!=0||counters[19]!=rebuilds+1||memcmp(actual,field,bytes)==0) {
        interaction_failure("rest-only cut failed to refresh geometry and distance caches"); goto done;
    }
    // Create one isolated material voxel in a loose body's atlas, then cut it.
    // This tests the third edit category without intersecting either city store.
    TestBody body={0}; body.position[0]=40; body.position[1]=20; body.position[2]=40;
    body.info[0]=body.info[1]=1;
    uint32_t atlas[64]={4},one=1;
    if(!begin_command()||!vg_dispatch(22)||!upload(4,&body,sizeof body,0)||!upload(5,atlas,sizeof atlas,0)||
       !upload(7,&one,4,0)||!upload(7,&one,4,17*4)||!upload(7,&one,4,21*4)||
       !interaction_reconstruct()||!finish(true)||!interaction_structure_read(before_structures)||
       !download(1,parents,bytes+4,0)||!download(2,field,bytes,0)) goto done;
    // A slightly enlarged conservative bound would be recomputed by material
    // analysis. Retaining it proves body edits skipped that analysis, rather
    // than coincidentally recomputing the same mass. Restore it before motion.
    float original_bound=before_structures[8].bounds_min[0];
    before_structures[8].bounds_min[0]-=.125f;
    params.edit[0]=params.edit[2]=38.5f; params.edit[1]=18.5f;
    if(!begin_command()||!vg_dispatch(22)||
       !upload(4,&before_structures[8],sizeof before_structures[8],BODY_CAPACITY*80u+8*sizeof(GPUStructure))||
       !upload(6,marker,sizeof marker,0)||!vg_dispatch(1)||
       !vg_dispatch(18)||!vg_dispatch(2)||!vg_dispatch(3)||!vg_dispatch(4)||!vg_dispatch(15)||!finish(true)||
       !download(7,counters,sizeof counters,0)||!download(1,after_parents,bytes+4,0)||
       !interaction_structure_read(after_structures)||!download(4,&body,sizeof body,0)||
       !download(6,probe,sizeof probe,0)) goto done;
    if(counters[18]!=4||counters[17]!=1||counters[21]!=1||body.info[0]!=0||body.info[1]!=0||
       memcmp(parents,after_parents,bytes+4)!=0||memcmp(before_structures,after_structures,sizeof before_structures)!=0||
       probe[0]!=0||probe[1]!=0||probe[2]!=0||probe[3]!=0) {
        interaction_failure("body-only cut failed to update its atlas/GI or mutated support/structure state"); goto done;
    }
    rebuilds=counters[19];
    before_structures[8].bounds_min[0]=original_bound;
    if(!begin_command()||!upload(4,&before_structures[8],sizeof before_structures[8],BODY_CAPACITY*80u+8*sizeof(GPUStructure))||
       !interaction_reconstruct()||!finish(true)||!download(2,actual,bytes,0)||
       !download(7,counters,sizeof counters,0)) goto done;
    if(counters[17]!=0||counters[21]!=0||counters[19]!=rebuilds+1||memcmp(actual,field,bytes)==0) {
        interaction_failure("body-only cut failed to refresh geometry and distance caches"); goto done;
    }
    // Pure legacy fixtures deliberately disable retained geometry. A zero dirty
    // flag must still dispatch full work and reconstruct their uploaded cells.
    memset(cells,0,bytes); cells[cell_at(3,3,3)]=2;
    if(!fixture(cells)||!begin_command()||!vg_dispatch(22)||!interaction_reconstruct()||!finish(true)||
       !download(7,counters,sizeof counters,0)||!download(2,actual,bytes,0)) goto done;
    if(counters[12]!=0||counters[17]!=0||counters[18]!=0||counters[21]!=0||counters[19]!=1||counters[26]!=1||
       actual[cell_at(3,3,3)]!=(1u<<16)||actual[cell_at(4,3,3)]!=1) {
        interaction_failure("legacy fixture with clean flags skipped required reconstruction"); goto done;
    }
    if(!interaction_geometry_dispatches(true,true)) goto done;
    fprintf(stderr,"GPU selftest: retained geometry, missed edits, dirty rebuilds and indirect extents pass\n");
    ok=true;
done:
    free(field); free(overlay); free(parents); free(after_parents);
    return ok;
}
