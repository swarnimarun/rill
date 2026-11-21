// SDL3 GPU resource and command adapter. Rill schedules the frame graph;
// material edits, support, debris, distance fields and image work run in MSL.
#include "gpu.h"
#include "input.h"
#include <SDL3/SDL.h>
#include <math.h>
#include <stddef.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>

_Static_assert(sizeof(GPUParams) == 192, "GPU uniform layout");
_Static_assert(offsetof(GPUParams, world) == 96, "GPU world offset");
_Static_assert(sizeof(GPUStructure)==128,"GPU structure layout");
_Static_assert(sizeof(GPUPlayer)==64,"GPU player layout");
#define PASSES 27
#define BODY_CAPACITY 8192u
#define PI 3.14159265358979323846f
static const char *entries[PASSES] = {
    "reset_world", "edit_world", "support_seed", "support_union", "detach_bodies",
    "integrate_bodies", "clear_overlay", "voxelize_bodies", "build_bricks",
    "distance_init", "distance_x", "distance_y", "distance_z", "update_probes",
    "render_world", "clear_probes", "pick_ray",
    "structure_init", "structure_analyze", "structure_integrate", "structure_voxelize", "player_step",
    "frame_begin", "prepare_geometry", "lighting_cache", "geometry_complete", "distance_complete"
};
static SDL_GPUDevice *device;
static SDL_Window *window;
static SDL_GPUCommandBuffer *command;
static SDL_GPUComputePipeline *pipelines[PASSES];
static SDL_GPUBuffer *buffers[8];
static SDL_GPUTexture *image;
static SDL_GPUBuffer *geometry_dispatch;
static bool indirect_prepared;
static GPUParams params;
static uint32_t sizes[8];
static int32_t keys[16];
static VoxelInput input;
static uint32_t width, height, dimension, frames, frame_limit;
static Uint64 last_tick, frame_start, title_tick;
static double elapsed_ns, max_ns;
static double *frame_samples;
static uint32_t sample_capacity;
static bool headless, smoke, preflight_failed, acquired_swapchain, input_fault;
static const char *presentation;

static uint32_t setting(const char *name, uint32_t fallback, uint32_t low, uint32_t high) {
    const char *s = getenv(name);
    if (!s || !*s) return fallback;
    char *end = NULL;
    unsigned long value = strtoul(s, &end, 10);
    if (*end || value < low || value > high) {
        fprintf(stderr, "%s must be between %u and %u\n", name, low, high);
        return 0;
    }
    return (uint32_t)value;
}
static bool enabled(const char *name) { const char *s = getenv(name); return s && strcmp(s,"1") == 0; }
static bool gpu_error(const char *what) {
    fprintf(stderr, "GPU %s: %s\n", what, SDL_GetError());
    return false;
}
static bool finish(bool wait) {
    if (!command) return false;
    SDL_GPUCommandBuffer *submitted = command;
    command = NULL;
    acquired_swapchain = false;
    if (!wait) return SDL_SubmitGPUCommandBuffer(submitted) || gpu_error("submit");
    SDL_GPUFence *fence = SDL_SubmitGPUCommandBufferAndAcquireFence(submitted);
    if (!fence) return gpu_error("submit fence");
    bool ok = SDL_WaitForGPUFences(device, true, &fence, 1);
    SDL_ReleaseGPUFence(device, fence);
    return ok || gpu_error("wait fence");
}
static bool begin_command(void) {
    indirect_prepared=false;
    command = SDL_AcquireGPUCommandBuffer(device);
    return command != NULL || gpu_error("command buffer");
}
static bool upload(uint32_t slot, const void *data, uint32_t bytes, uint32_t offset) {
    SDL_GPUTransferBufferCreateInfo ci = {.usage=SDL_GPU_TRANSFERBUFFERUSAGE_UPLOAD,.size=bytes};
    SDL_GPUTransferBuffer *transfer = SDL_CreateGPUTransferBuffer(device,&ci);
    if (!transfer) return gpu_error("upload transfer");
    void *mapped = SDL_MapGPUTransferBuffer(device,transfer,false);
    if (!mapped) { SDL_ReleaseGPUTransferBuffer(device,transfer); return gpu_error("upload map"); }
    memcpy(mapped,data,bytes);
    SDL_UnmapGPUTransferBuffer(device,transfer);
    SDL_GPUCopyPass *copy = SDL_BeginGPUCopyPass(command);
    SDL_GPUTransferBufferLocation source = {.transfer_buffer=transfer};
    SDL_GPUBufferRegion destination = {.buffer=buffers[slot],.offset=offset,.size=bytes};
    SDL_UploadToGPUBuffer(copy,&source,&destination,false);
    SDL_EndGPUCopyPass(copy);
    SDL_ReleaseGPUTransferBuffer(device,transfer);
    return true;
}
static bool download(uint32_t slot, void *destination, uint32_t bytes, uint32_t offset) {
    if (!begin_command()) return false;
    SDL_GPUTransferBufferCreateInfo ci = {.usage=SDL_GPU_TRANSFERBUFFERUSAGE_DOWNLOAD,.size=bytes};
    SDL_GPUTransferBuffer *transfer = SDL_CreateGPUTransferBuffer(device,&ci);
    if (!transfer) return gpu_error("download transfer");
    SDL_GPUCopyPass *copy = SDL_BeginGPUCopyPass(command);
    SDL_GPUBufferRegion source = {.buffer=buffers[slot],.offset=offset,.size=bytes};
    SDL_GPUTransferBufferLocation target = {.transfer_buffer=transfer};
    SDL_DownloadFromGPUBuffer(copy,&source,&target);
    SDL_EndGPUCopyPass(copy);
    bool ok = finish(true);
    if (ok) {
        void *mapped = SDL_MapGPUTransferBuffer(device,transfer,false);
        if (mapped) { memcpy(destination,mapped,bytes); SDL_UnmapGPUTransferBuffer(device,transfer); }
        else ok = gpu_error("download map");
    }
    SDL_ReleaseGPUTransferBuffer(device,transfer);
    return ok;
}
static bool capture(const char *path) {
    SDL_GPUTransferBufferCreateInfo ci = {.usage=SDL_GPU_TRANSFERBUFFERUSAGE_DOWNLOAD,.size=width*height*4};
    SDL_GPUTransferBuffer *transfer = SDL_CreateGPUTransferBuffer(device,&ci);
    if (!transfer) return gpu_error("image transfer");
    SDL_GPUCopyPass *copy = SDL_BeginGPUCopyPass(command);
    SDL_GPUTextureRegion source = {.texture=image,.w=width,.h=height,.d=1};
    SDL_GPUTextureTransferInfo target = {.transfer_buffer=transfer};
    SDL_DownloadFromGPUTexture(copy,&source,&target);
    SDL_EndGPUCopyPass(copy);
    bool ok = finish(true);
    if (ok) {
        void *mapped = SDL_MapGPUTransferBuffer(device,transfer,false);
        SDL_Surface *surface = mapped ? SDL_CreateSurfaceFrom((int)width,(int)height,SDL_PIXELFORMAT_RGBA32,mapped,(int)width*4) : NULL;
        ok = surface && SDL_SaveBMP(surface,path);
        if (surface) SDL_DestroySurface(surface);
        if (mapped) SDL_UnmapGPUTransferBuffer(device,transfer);
        if (!ok) gpu_error("save image");
    }
    SDL_ReleaseGPUTransferBuffer(device,transfer);
    return ok;
}
static char *load_shaders(void) {
    const char *directory = getenv("RILL_VOXEL_GPU_SHADERS");
    if (!directory) { fprintf(stderr,"Set RILL_VOXEL_GPU_SHADERS or use the launcher\n"); return NULL; }
    const char *names[] = {"shared.metal","simulation.metal","structural.metal","player.metal","dispatch.metal","tracing.metal","render.metal"};
    char *all = NULL;
    size_t used = 0;
    for (unsigned i=0;i<sizeof names/sizeof names[0];i++) {
        char path[4096];
        int n = snprintf(path,sizeof path,"%s/%s",directory,names[i]);
        if (n<0 || (size_t)n>=sizeof path) { free(all); return NULL; }
        size_t length = 0;
        void *source = SDL_LoadFile(path,&length);
        if (!source) { gpu_error(path); free(all); return NULL; }
        char *grown = realloc(all,used+length+2);
        if (!grown) { SDL_free(source); free(all); return NULL; }
        all = grown;
        memcpy(all+used,source,length); used += length;
        all[used++]='\n'; all[used]=0;
        SDL_free(source);
    }
    return all;
}
static bool build_pipelines(void) {
    char *source = load_shaders();
    if (!source) return false;
    size_t length = strlen(source);
    for (unsigned i=0;i<PASSES;i++) {
        SDL_GPUComputePipelineCreateInfo ci = {
            .code_size=length,.code=(const Uint8*)source,.entrypoint=entries[i],
            .format=SDL_GPU_SHADERFORMAT_MSL,.num_readwrite_storage_buffers=i==23 ? 2u:8u,
            .num_readwrite_storage_textures=i==14 ? 1u:0u,.num_uniform_buffers=1,
            .threadcount_x=(i==14||i==24) ? 8u:64u,.threadcount_y=(i==14||i==24) ? 8u:1u,.threadcount_z=1
        };
        pipelines[i] = SDL_CreateGPUComputePipeline(device,&ci);
        if (!pipelines[i]) { gpu_error(entries[i]); free(source); return false; }
    }
    free(source);
    return true;
}
static bool allocate_storage(void) {
    uint32_t count = dimension*dimension*dimension, probe = dimension/4, brick = dimension/8;
    sizes[0]=count*8; sizes[1]=(count*2+1)*4; sizes[2]=count*4;
    sizes[3]=brick*brick*brick*4; sizes[4]=BODY_CAPACITY*80+9*sizeof(GPUStructure)+sizeof(GPUPlayer);
    sizes[5]=BODY_CAPACITY*64*4;
    const char *gi=getenv("RILL_VOXEL_GPU_GI");
    if(gi && strcmp(gi,"balanced")!=0 && strcmp(gi,"full")!=0) { fprintf(stderr,"RILL_VOXEL_GPU_GI must be balanced or full\n"); return false; }
    params.limits[2]=gi && strcmp(gi,"full")==0 ? 1u:2u;
    uint32_t cache_pixels=params.limits[2]>1 ? ((width+1)/2)*((height+1)/2):0;
    sizes[6]=2*probe*probe*probe*16+cache_pixels*32; sizes[7]=64*4;
    for (unsigned i=0;i<8;i++) {
        SDL_GPUBufferCreateInfo ci = {.usage=SDL_GPU_BUFFERUSAGE_COMPUTE_STORAGE_READ|SDL_GPU_BUFFERUSAGE_COMPUTE_STORAGE_WRITE,.size=sizes[i]};
        buffers[i]=SDL_CreateGPUBuffer(device,&ci);
        if (!buffers[i]) return gpu_error("storage buffer");
    }
    SDL_GPUBufferCreateInfo indirect_info={.usage=SDL_GPU_BUFFERUSAGE_INDIRECT|SDL_GPU_BUFFERUSAGE_COMPUTE_STORAGE_WRITE,.size=8*12};
    geometry_dispatch=SDL_CreateGPUBuffer(device,&indirect_info);
    if(!geometry_dispatch) return gpu_error("indirect geometry dispatch");
    SDL_GPUTextureCreateInfo ti = {.type=SDL_GPU_TEXTURETYPE_2D,.format=SDL_GPU_TEXTUREFORMAT_R8G8B8A8_UNORM,
        .usage=SDL_GPU_TEXTUREUSAGE_COMPUTE_STORAGE_WRITE|SDL_GPU_TEXTUREUSAGE_SAMPLER,
        .width=width,.height=height,.layer_count_or_depth=1,.num_levels=1,.sample_count=SDL_GPU_SAMPLECOUNT_1};
    image=SDL_CreateGPUTexture(device,&ti);
    if (!image) return gpu_error("output texture");
    params.world[0]=params.world[1]=params.world[2]=dimension; params.world[3]=count;
    params.limits[0]=BODY_CAPACITY; params.limits[1]=probe; params.limits[3]=8;
    params.config[0]=brick; params.config[1]=width; params.config[2]=height; params.config[3]=4;
    return true;
}
bool vg_open(void) {
    preflight_failed=false;
    dimension=setting("RILL_VOXEL_GPU_WORLD",128,64,256);
    width=setting("RILL_VOXEL_GPU_WIDTH",1280,1,4096);
    height=setting("RILL_VOXEL_GPU_HEIGHT",720,1,4096);
    frame_limit=setting("RILL_VOXEL_GPU_FRAMES",0,1,100000);
    if ((dimension!=64 && dimension!=128 && dimension!=256) || !width || !height ||
        (getenv("RILL_VOXEL_GPU_FRAMES") && frame_limit==0)) return false;
    headless=enabled("RILL_VOXEL_GPU_HEADLESS");
    smoke=enabled("RILL_VOXEL_GPU_SMOKE")||enabled("RILL_VOXEL_GPU_BENCHMARK");
    frames=0; elapsed_ns=0; max_ns=0;
    sample_capacity=frame_limit;
    frame_samples=sample_capacity?calloc(sample_capacity,sizeof *frame_samples):NULL;
    if(sample_capacity && !frame_samples) return false;
    memset(&input,0,sizeof input); input_fault=false;
    memset(&params,0,sizeof params);
    if (!SDL_Init(SDL_INIT_VIDEO)) { preflight_failed=true; return gpu_error("video initialization"); }
    device=SDL_CreateGPUDevice(SDL_GPU_SHADERFORMAT_MSL,enabled("RILL_VOXEL_GPU_DEBUG"),"metal");
    if (!device) { preflight_failed=true; return gpu_error("Metal device unavailable"); }
    presentation="offscreen";
    if (!headless) {
        window=SDL_CreateWindow("Rill GPU voxel lab | Click to play | WASD walk, Space jump, Ctrl sneak, Shift sprint, F fly, Esc cursor, X demolish",1280,720,SDL_WINDOW_RESIZABLE);
        if (!window || !SDL_ClaimWindowForGPUDevice(device,window)) return gpu_error("window claim");
        SDL_GPUPresentMode present=SDL_GPU_PRESENTMODE_VSYNC;
        if(!enabled("RILL_VOXEL_GPU_VSYNC") && SDL_WindowSupportsGPUPresentMode(device,window,SDL_GPU_PRESENTMODE_IMMEDIATE))
            present=SDL_GPU_PRESENTMODE_IMMEDIATE;
        if (!SDL_SetGPUSwapchainParameters(device,window,SDL_GPU_SWAPCHAINCOMPOSITION_SDR,present)) return gpu_error("swapchain");
        presentation=present==SDL_GPU_PRESENTMODE_IMMEDIATE?"immediate":"vsync";
    }
    if (!allocate_storage() || !build_pipelines()) return false;
    input.focused=window && (SDL_GetWindowFlags(window)&SDL_WINDOW_INPUT_FOCUS)!=0;
    last_tick=SDL_GetTicks(); title_tick=last_tick;
    fprintf(stderr,"SDL3 GPU backend=%s world=%u^3 (%u voxels) storage=%.1f MiB\n",SDL_GetGPUDeviceDriver(device),dimension,params.world[3],
        (double)(sizes[0]+sizes[1]+sizes[2]+sizes[3]+sizes[4]+sizes[5]+sizes[6]+sizes[7])/(1024.0*1024.0));
    return true;
}

static void apply_capture(CaptureAction action) {
    if(!window || (action!=CAPTURE_ENABLE && action!=CAPTURE_DISABLE)) return;
    if(!SDL_SetWindowRelativeMouseMode(window,action==CAPTURE_ENABLE)) {
        gpu_error("relative mouse mode"); vi_release(&input); input_fault=true;
    }
    last_tick=SDL_GetTicks();
}
bool vg_poll(void) {
    memset(keys,0,sizeof keys); vi_begin(&input);
    bool running=true;
    SDL_Event event;
    while (SDL_PollEvent(&event)) {
        if (event.type==SDL_EVENT_QUIT || event.type==SDL_EVENT_WINDOW_CLOSE_REQUESTED) running=false;
        if(event.type==SDL_EVENT_WINDOW_FOCUS_LOST) { memset(keys,0,sizeof keys); apply_capture(vi_focus(&input,false)); }
        if(event.type==SDL_EVENT_WINDOW_FOCUS_GAINED) { memset(keys,0,sizeof keys); apply_capture(vi_focus(&input,true)); last_tick=SDL_GetTicks(); }
        if (event.type==SDL_EVENT_KEY_DOWN && !event.key.repeat && input.focused) {
            switch (event.key.scancode) {
            case SDL_SCANCODE_ESCAPE: apply_capture(vi_escape(&input,true)); break;
            case SDL_SCANCODE_TAB: apply_capture(vi_escape(&input,false)); break;
            case SDL_SCANCODE_R: keys[8]=1; break;
            case SDL_SCANCODE_X: keys[9]=1; break;
            case SDL_SCANCODE_P: keys[10]=1; break;
            case SDL_SCANCODE_G: keys[11]=1; break;
            case SDL_SCANCODE_T: keys[13]=1; break;
            case SDL_SCANCODE_F: if(input.captured) keys[12]=1; break;
            case SDL_SCANCODE_SPACE: if(input.captured) input.jump_pressed=true; break;
            default: break;
            }
        }
        if(event.type==SDL_EVENT_MOUSE_BUTTON_DOWN || event.type==SDL_EVENT_MOUSE_BUTTON_UP) {
            if(event.button.button==SDL_BUTTON_LEFT || event.button.button==SDL_BUTTON_RIGHT)
                apply_capture(vi_click(&input,event.button.button==SDL_BUTTON_LEFT,event.type==SDL_EVENT_MOUSE_BUTTON_DOWN));
        }
        if(event.type==SDL_EVENT_MOUSE_MOTION && isfinite(event.motion.xrel) && isfinite(event.motion.yrel))
            vi_motion(&input,(int32_t)fmaxf(-1024,fminf(1024,event.motion.xrel)),(int32_t)fmaxf(-1024,fminf(1024,event.motion.yrel)));
    }
    if(input.focused && input.captured) {
        const bool *keyboard=SDL_GetKeyboardState(NULL);
        keys[0]=keyboard[SDL_SCANCODE_W]; keys[1]=keyboard[SDL_SCANCODE_S];
        keys[2]=keyboard[SDL_SCANCODE_A]; keys[3]=keyboard[SDL_SCANCODE_D];
        keys[4]=keyboard[SDL_SCANCODE_SPACE]||input.jump_pressed;
        keys[5]=keyboard[SDL_SCANCODE_LCTRL]||keyboard[SDL_SCANCODE_RCTRL];
        keys[6]=keyboard[SDL_SCANCODE_LSHIFT]||keyboard[SDL_SCANCODE_RSHIFT];
        keys[7]=!input.just_captured && (input.fire_pressed||input.fire_down);
        vi_motion(&input,2*((int)keyboard[SDL_SCANCODE_RIGHT]-(int)keyboard[SDL_SCANCODE_LEFT]),
            2*((int)keyboard[SDL_SCANCODE_DOWN]-(int)keyboard[SDL_SCANCODE_UP]));
    }
    return running && !input.quit && (frame_limit==0 || frames<frame_limit);
}
int32_t vg_key(int32_t key) {
    if(smoke && enabled("RILL_VOXEL_GPU_STRESS")) {
        if(key==7) return 1;
        if(key==0) return frames%180<120 ? 1:0;
        if(key==2) return frames%180>=120 ? 1:0;
    }
    return key>=0 && key<16 ? keys[key]:0;
}
int32_t vg_mouse(int32_t axis) { return axis==0 || axis==1 ? input.motion[axis]:0; }
int32_t vg_elapsed(void) {
    Uint64 now=SDL_GetTicks(), elapsed=now-last_tick; last_tick=now;
    return smoke ? 16 : elapsed>50 ? 50 : (int32_t)elapsed;
}
int32_t vg_world(void) { return (int32_t)dimension; }
int32_t vg_sine(int32_t angle) { return (int32_t)(sinf((float)angle*PI/180000.0f)*1024.0f); }
int32_t vg_cosine(int32_t angle) { return (int32_t)(cosf((float)angle*PI/180000.0f)*1024.0f); }
bool vg_smoke(void) { return smoke; }
bool vg_automatic_demo(void) { return !enabled("RILL_VOXEL_GPU_NO_DEMO"); }
int32_t vg_mode(void) { return smoke && !enabled("RILL_VOXEL_GPU_WALK") ? 0:1; }
bool vg_begin(int32_t x,int32_t y,int32_t z,int32_t yaw,int32_t pitch,int32_t elapsed,int32_t frame,int32_t view) {
    if (command || input_fault) return false;
    frame_start=SDL_GetTicksNS();
    float a=(float)yaw*PI/180000.0f, b=(float)pitch*PI/180000.0f;
    params.eye[0]=(float)x/1024; params.eye[1]=(float)y/1024; params.eye[2]=(float)z/1024; params.eye[3]=0.68f;
    params.forward[0]=sinf(a)*cosf(b); params.forward[1]=sinf(b); params.forward[2]=-cosf(a)*cosf(b); params.forward[3]=(float)width/(float)height;
    params.right[0]=cosf(a); params.right[1]=0; params.right[2]=sinf(a);
    params.up[0]=-sinf(a)*sinf(b); params.up[1]=cosf(b); params.up[2]=cosf(a)*sinf(b);
    params.up[3]=(float)frame/60;
    params.dynamics[0]=smoke ? 1.0f/60.0f : fminf(0.05f,fmaxf(0.001f,(float)elapsed/1000));
    params.dynamics[1]=24; params.dynamics[2]=30; params.dynamics[3]=1;
    params.frame[0]=(uint32_t)frame; params.frame[1]=(uint32_t)view;
    params.frame[3]=0; params.edit[3]=0;
    return begin_command();
}
bool vg_dispatch(int32_t pass) {
    if (!command || pass<0 || pass>=PASSES) return false;
    if (pass==0) {
        uint32_t zero[64]={0};
        if (!upload(7,zero,sizeof zero,0)) return false;
    }
    if(pass==23) {
        SDL_GPUStorageBufferReadWriteBinding prep[2]={{.buffer=buffers[7]},{.buffer=geometry_dispatch}};
        SDL_PushGPUComputeUniformData(command,0,&params,sizeof params);
        SDL_GPUComputePass *compute=SDL_BeginGPUComputePass(command,NULL,0,prep,2);
        if(!compute) return gpu_error("prepare geometry pass");
        SDL_BindGPUComputePipeline(compute,pipelines[23]);
        SDL_DispatchGPUCompute(compute,1,1,1);
        SDL_EndGPUComputePass(compute); indirect_prepared=true;
        return true;
    }
    if(pass==0||pass==1||pass==4||pass==5||pass==17||pass==18||pass==19||pass==22) indirect_prepared=false;
    SDL_GPUStorageBufferReadWriteBinding bindings[8]={0};
    for (unsigned i=0;i<8;i++) bindings[i].buffer=buffers[i];
    SDL_GPUStorageTextureReadWriteBinding output={.texture=image};
    SDL_PushGPUComputeUniformData(command,0,&params,sizeof params);
    SDL_GPUComputePass *compute=SDL_BeginGPUComputePass(command,pass==14?&output:NULL,pass==14?1:0,bindings,8);
    if (!compute) return gpu_error("begin compute pass");
    SDL_BindGPUComputePipeline(compute,pipelines[pass]);
    uint32_t count=params.world[3];
    if (pass==1) count+=BODY_CAPACITY;
    if (pass==4) count=(dimension/4)*(dimension/4)*(dimension/4);
    if (pass==5) count=BODY_CAPACITY;
    if (pass==7) count=BODY_CAPACITY*64;
    if (pass==8) count=params.config[0]*params.config[0]*params.config[0];
    if (pass>=10 && pass<=12) count=dimension*dimension;
    if (pass==13) count=params.limits[1]*params.limits[1]*params.limits[1];
    if (pass==15) count=2*params.limits[1]*params.limits[1]*params.limits[1];
    if (pass==16 || pass==21 || pass==22 || pass==25 || pass==26) count=1;
    if (pass==18) count=9*64;
    if (pass==19) count=9;
    int slot=-1;
    const int geometry_passes[8]={6,7,20,8,9,10,11,12};
    for(int i=0;i<8;i++) if(pass==geometry_passes[i]) slot=i;
    if (pass==24) {
        uint32_t stride=params.limits[2];
        if(stride>1) SDL_DispatchGPUCompute(compute,((width+stride-1)/stride+7)/8,((height+stride-1)/stride+7)/8,1);
        else SDL_DispatchGPUCompute(compute,1,1,1);
    }
    else if (pass==14) SDL_DispatchGPUCompute(compute,(width+7)/8,(height+7)/8,1);
    else if(indirect_prepared && slot>=0) SDL_DispatchGPUComputeIndirect(compute,geometry_dispatch,(Uint32)slot*12);
    else SDL_DispatchGPUCompute(compute,(count+63)/64,1,1);
    SDL_EndGPUComputePass(compute);
    return true;
}
bool vg_controls(int32_t mode, bool reset, bool paused) {
    if(!command || mode<0 || mode>2) return false;
    params.movement[0]=(float)(vg_key(3)-vg_key(2));
    params.movement[1]=(float)(vg_key(0)-vg_key(1));
    params.movement[2]=(float)keys[4]; params.movement[3]=(float)keys[5];
    params.controller[0]=(uint32_t)mode; params.controller[1]=reset?1u:0u;
    params.controller[2]=keys[6]?1u:0u; params.controller[3]=(input.captured&&input.focused)||(smoke&&enabled("RILL_VOXEL_GPU_STRESS"))?1u:0u;
    if(paused) { params.dynamics[0]=0; params.controller[3]=0; }
    return true;
}
bool vg_aim(int32_t radius) {
    if (radius<1 || radius>32) return false;
    if(smoke && enabled("RILL_VOXEL_GPU_STRESS")) {
        uint32_t tile=dimension/3,story=dimension/12;
        if(story<5) story=5;
        uint32_t shot=frames/7, margin=dimension/32;
        if(margin<3) margin=3;
        params.frame[3]=0;
        params.edit[0]=(float)(tile+margin+1)+.5f;
        params.edit[1]=(float)(story+(shot%3)*story)+.5f;
        params.edit[2]=(float)(2*tile+margin+3+(shot/3)%((tile>10)?tile-10:1))+.5f;
        params.edit[3]=2;
        return true;
    }
    params.frame[3]=1; params.edit[3]=(float)radius;
    return vg_dispatch(16);
}
bool vg_lean(void) {
    params.frame[3]=0;
    uint32_t tile=dimension/3,story=dimension/12;
    if(story<5) story=5;
    params.edit[0]=(float)(tile+tile/2)-.5f;
    params.edit[1]=(float)(story/2)+.5f;
    params.edit[2]=(float)(2*tile+tile/2)+.5f;
    params.edit[3]=1;
    return command!=NULL;
}
bool vg_demo(int32_t radius) {
    if (radius<1 || radius>32) return false;
    if(enabled("RILL_VOXEL_GPU_LEAN")) return vg_lean();
    params.frame[3]=0;
    float tile=(float)(dimension/3);
    params.edit[0]=floorf(tile/2)+tile; params.edit[1]=(float)dimension/24; params.edit[2]=floorf(tile/2)+2*tile;
    params.edit[3]=(float)radius;
    return true;
}
bool vg_present(void) {
    if (!command) return false;
    if (window) {
        SDL_GPUTexture *swap=NULL;
        Uint32 w=0,h=0;
        if (!SDL_WaitAndAcquireGPUSwapchainTexture(command,window,&swap,&w,&h)) return gpu_error("acquire swapchain");
        if (swap) {
            acquired_swapchain=true;
            SDL_GPUBlitInfo blit={.source={.texture=image,.w=width,.h=height},
                .destination={.texture=swap,.w=w,.h=h},.load_op=SDL_GPU_LOADOP_DONT_CARE,.filter=SDL_GPU_FILTER_LINEAR};
            SDL_BlitGPUTexture(command,&blit);
        }
    }
    const char *path=getenv("RILL_VOXEL_GPU_CAPTURE");
    bool last=frame_limit!=0 && frames+1==frame_limit;
    bool ok=path && last ? capture(path):finish(smoke);
    if (!ok) return false;
    double duration=(double)(SDL_GetTicksNS()-frame_start);
    elapsed_ns+=duration; if (duration>max_ns) max_ns=duration;
    if(frame_samples && frames<sample_capacity) frame_samples[frames]=duration/1e6;
    ++frames;
    Uint64 now=SDL_GetTicks();
    if (window && now-title_tick>=1000) {
        char title[256];
        snprintf(title,sizeof title,"Rill GPU | %u^3 | %.1f ms | WASD walk / mouse look / LMB blast / T lean / X demolish / F fly / Esc cursor",dimension,elapsed_ns/(double)frames/1e6);
        SDL_SetWindowTitle(window,title); title_tick=now;
    }
    return true;
}
static int compare_frame_times(const void *a,const void *b) {
    double x=*(const double*)a,y=*(const double*)b;
    return (x>y)-(x<y);
}
void vg_close(void) {
    if (device) {
        if (command) {
            if (acquired_swapchain) finish(true);
            else { SDL_CancelGPUCommandBuffer(command); command=NULL; }
        }
        SDL_WaitForGPUIdle(device);
        if (frames) {
            uint32_t counters[64]={0};
            download(7,counters,sizeof counters,0);
            uint32_t warmup=frame_samples?(frames/4<20?frames/4:20):0, sample_count=frames-warmup;
            double steady_ms=elapsed_ns/(double)frames/1e6;
            char p95_json[32]="null";
            if(frame_samples && sample_count && frames<=sample_capacity) {
                double sum=0;
                for(uint32_t i=warmup;i<frames;i++) sum+=frame_samples[i];
                steady_ms=sum/sample_count;
                qsort(frame_samples+warmup,sample_count,sizeof *frame_samples,compare_frame_times);
                uint32_t rank=(sample_count*95+99)/100;
                snprintf(p95_json,sizeof p95_json,"%.4f",frame_samples[warmup+(rank?rank-1:0)]);
            }
            fprintf(stderr,"{\"backend\":\"%s\",\"world\":%u,\"voxels\":%u,\"frames\":%u,\"render_width\":%u,\"render_height\":%u,\"frame_ms\":%.4f,\"max_frame_ms\":%.4f,\"bodies\":%u,\"removed\":%u,\"detached\":%u,\"capacity_overflow\":%u,\"support_error\":%u,\"trace_budget\":%u,\"structures\":%u,\"structure_voxels\":%u,\"tilted_mask\":%u,\"unsupported_mask\":%u,\"player_recoveries\":%u,\"player_stuck\":%u,\"geometry_rebuilds\":%u,\"distance_rebuilds\":%u,\"gi_stride\":%u,\"steady_frame_ms\":%.4f,\"p95_frame_ms\":%s,\"warmup_frames\":%u,\"synchronous_timing\":%s,\"presentation\":\"%s\"}\n",
                SDL_GetGPUDeviceDriver(device),dimension,params.world[3],frames,width,height,elapsed_ns/(double)frames/1e6,max_ns/1e6,counters[0],counters[1],counters[2],counters[4],counters[3],counters[7],counters[13],counters[14],counters[15],counters[16],counters[24],counters[25],counters[19],counters[26],params.limits[2],steady_ms,p95_json,warmup,smoke?"true":"false",presentation);
        }
        for (unsigned i=0;i<PASSES;i++) if (pipelines[i]) { SDL_ReleaseGPUComputePipeline(device,pipelines[i]); pipelines[i]=NULL; }
        for (unsigned i=0;i<8;i++) if (buffers[i]) { SDL_ReleaseGPUBuffer(device,buffers[i]); buffers[i]=NULL; }
        if (geometry_dispatch) { SDL_ReleaseGPUBuffer(device,geometry_dispatch); geometry_dispatch=NULL; }
        if (image) { SDL_ReleaseGPUTexture(device,image); image=NULL; }
        if (window) { SDL_SetWindowRelativeMouseMode(window,false); SDL_ReleaseWindowFromGPUDevice(device,window); }
        SDL_DestroyGPUDevice(device); device=NULL;
    }
    if (window) { SDL_DestroyWindow(window); window=NULL; }
    free(frame_samples); frame_samples=NULL; sample_capacity=0;
    SDL_Quit();
}

// Native GPU verification uploads fixtures only in this test entry point.
// Production never reads bulk world state back to run its simulation on the CPU.
static bool fixture(const uint32_t *cells) {
    uint32_t zero[64]={0};
    return begin_command() && upload(0,cells,params.world[3]*4,0) && upload(7,zero,sizeof zero,0) &&
        vg_dispatch(6) && vg_dispatch(15) && finish(true);
}
static bool distance_passes(void) {
    return begin_command() && vg_dispatch(9) && vg_dispatch(10) && vg_dispatch(11) && vg_dispatch(12) && finish(true);
}
static uint32_t cell_at(unsigned x,unsigned y,unsigned z) { return x+dimension*(y+dimension*z); }
static unsigned absolute_i(int value) { return (unsigned)(value<0?-value:value); }
static bool test_distances(uint32_t *cells,uint32_t *field) {
    uint32_t count=params.world[3];
    memset(cells,0,params.world[3]*4);
    uint32_t points[3][3]={{1,6,2},{dimension-3,1,dimension-5},{dimension/2,dimension/2,dimension/2}};
    for(unsigned k=0;k<3;k++) cells[cell_at(points[k][0],points[k][1],points[k][2])]=k+2;
    if(!fixture(cells)||!distance_passes()||!download(2,field,sizes[2],0)) return false;
    for(uint32_t i=0;i<count;i++) {
        unsigned x=i%dimension,y=i/dimension%dimension,z=i/(dimension*dimension), best=65535;
        for(unsigned k=0;k<3;k++) {
            unsigned d=absolute_i((int)x-(int)points[k][0])+absolute_i((int)y-(int)points[k][1])+absolute_i((int)z-(int)points[k][2]);
            if(d<best) best=d;
        }
        uint32_t expected=best|((cells[i]!=0?1u:0u)<<16);
        if(field[i]!=expected) { fprintf(stderr,"distance mismatch %u: %u expected %u\n",i,field[i],expected); return false; }
    }
    for(uint32_t i=0;i<count;i++) cells[i]=2;
    if(!fixture(cells)||!distance_passes()||!download(2,field,sizes[2],0)) return false;
    for(uint32_t i=0;i<count;i++) {
        unsigned x=i%dimension,y=i/dimension%dimension,z=i/(dimension*dimension), d=x+1;
        unsigned ds[]={y+1,z+1,dimension-x,dimension-y,dimension-z};
        for(unsigned k=0;k<5;k++) if(ds[k]<d) d=ds[k];
        if(field[i]!=(d<<16)) { fprintf(stderr,"solid exterior distance mismatch %u\n",i); return false; }
    }
    memset(cells,0,params.world[3]*4);
    if(!fixture(cells)||!distance_passes()||!download(2,field,sizes[2],0)) return false;
    for(uint32_t i=0;i<count;i++) if(field[i]!=65535) { fprintf(stderr,"empty distance mismatch\n"); return false; }
    fprintf(stderr,"GPU selftest: all %u signed distance entries match independent references\n",count);
    return true;
}
static bool test_edit(uint32_t *cells,uint32_t *actual) {
    uint32_t count=params.world[3];
    for(uint32_t i=0;i<count;i++) cells[i]=((i/dimension)%dimension)==0?1:2;
    if(!fixture(cells)||!begin_command()) return false;
    params.frame[3]=0; params.edit[0]=2.5f; params.edit[1]=1.5f; params.edit[2]=3.5f; params.edit[3]=4;
    if(!vg_dispatch(1)||!finish(true)||!download(0,actual,params.world[3]*4,0)) return false;
    unsigned removed=0;
    for(uint32_t i=0;i<count;i++) {
        float dx=(float)(i%dimension)-2,dy=(float)(i/dimension%dimension)-1,dz=(float)(i/(dimension*dimension))-3;
        uint32_t expected=cells[i];
        if(expected>1 && dx*dx+dy*dy+dz*dz<=16) { expected=0; ++removed; }
        if(actual[i]!=expected) { fprintf(stderr,"blast mismatch %u\n",i); return false; }
    }
    uint32_t counters[64];
    if(!download(7,counters,sizeof counters,0)||counters[1]!=removed) return false;
    fprintf(stderr,"GPU selftest: clipped sphere and bedrock preserve %u cells, remove %u\n",count-removed,removed);
    return true;
}
typedef struct TestBody {
    float position[4],velocity[4],angle[4],spin[4];
    uint32_t info[4];
} TestBody;
_Static_assert(sizeof(TestBody)==80,"GPU body readback layout");
static bool test_support(uint32_t *cells,uint32_t *actual) {
    memset(cells,0,params.world[3]*4);
    cells[cell_at(8,0,8)]=1;
    for(unsigned y=1;y<=7;y++) cells[cell_at(8,y,8)]=2;
    for(unsigned x=9;x<=15;x++) cells[cell_at(x,7,8)]=3;
    cells[cell_at(30,20,30)]=2; cells[cell_at(30,21,30)]=4;
    if(!fixture(cells)||!begin_command()||!vg_dispatch(2)||!vg_dispatch(3)||!finish(true)) return false;
    uint32_t *parents=malloc((params.world[3]+1)*4);
    if(!parents) return false;
    bool ok=download(1,parents,(params.world[3]+1)*4,0);
    for(uint32_t i=0;ok&&i<params.world[3];i++) if(cells[i]) {
        uint32_t root=i+1,steps=0;
        while(parents[root]!=root && steps++<params.world[3]) root=parents[root];
        bool anchored=i!=cell_at(30,20,30)&&i!=cell_at(30,21,30);
        if((root==0)!=anchored || steps>=params.world[3]) { fprintf(stderr,"support mismatch %u\n",i); ok=false; }
    }
    free(parents);
    if(!ok||!begin_command()||!vg_dispatch(4)||!finish(true)||!download(0,actual,params.world[3]*4,0)) return false;
    uint32_t counters[64];
    if(!download(7,counters,sizeof counters,0)||counters[3]!=0||counters[2]!=2||counters[0]!=1) { fprintf(stderr,"detach counters mismatch\n"); return false; }
    for(uint32_t i=0;i<params.world[3];i++) {
        uint32_t expected=i==cell_at(30,20,30)||i==cell_at(30,21,30)?0:cells[i];
        if(actual[i]!=expected) return false;
    }
    uint32_t atlas[64];
    if(!download(5,atlas,sizeof atlas,0)) return false;
    unsigned mass=0,sum=0;
    for(unsigned i=0;i<64;i++) { mass+=atlas[i]!=0; sum+=atlas[i]; }
    if(mass!=2||sum!=6) return false;
    TestBody before,after;
    if(!download(4,&before,sizeof before,0)) return false;
    // Real GPU integration and voxelization must preserve the two atlas voxels.
    params.dynamics[0]=1.0f/60; params.dynamics[1]=24; params.dynamics[2]=30;
    for(unsigned tick=0;tick<8;tick++) if(!begin_command()||!vg_dispatch(5)||!vg_dispatch(6)||!vg_dispatch(7)||!finish(true)) return false;
    if(!download(5,atlas,sizeof atlas,0)) return false;
    mass=sum=0; for(unsigned i=0;i<64;i++) { mass+=atlas[i]!=0; sum+=atlas[i]; }
    if(mass!=2||sum!=6||!download(4,&after,sizeof after,0)) return false;
    if(after.info[0]!=2 || after.info[1]!=1 || !isfinite(after.position[0]) ||
       !isfinite(after.position[1]) || !isfinite(after.position[2]) ||
       (before.position[0]==after.position[0] && before.position[1]==after.position[1] && before.position[2]==after.position[2])) {
        fprintf(stderr,"rigid fragment did not move correctly\n"); return false;
    }
    // Sleeping debris must wake when the static support beneath it disappears.
    after.info[3]=1; memset(after.velocity,0,sizeof after.velocity); memset(after.spin,0,sizeof after.spin);
    if(!begin_command()||!upload(4,&after,sizeof after,0)||!vg_dispatch(5)||!finish(true)||!download(4,&before,sizeof before,0)) return false;
    if(before.info[3]!=0 || !(before.position[1]<after.position[1])) {
        fprintf(stderr,"unsupported sleeping body failed to wake\n"); return false;
    }
    // With a zero body budget, unsupported material must remain in the world.
    if(!fixture(cells)||!begin_command()||!vg_dispatch(2)||!vg_dispatch(3)) return false;
    uint32_t capacity=params.limits[0]; params.limits[0]=0;
    ok=vg_dispatch(4)&&finish(true)&&download(0,actual,params.world[3]*4,0);
    params.limits[0]=capacity;
    if(!ok||memcmp(actual,cells,params.world[3]*4)!=0) { fprintf(stderr,"capacity exhausted lost material\n"); return false; }
    fprintf(stderr,"GPU selftest: exact support, detached mass, moving atlas and capacity retention pass\n");
    return true;
}
#include "interaction_tests.c"

int32_t vg_test(void) {
    bool ready=vg_open();
    if(!ready) { int32_t result=preflight_failed?77:1; vg_close(); return result; }
    uint32_t *cells=calloc(params.world[3],4), *actual=malloc(params.world[3]*4);
    bool ok=cells&&actual&&test_distances(cells,actual)&&test_edit(cells,actual)&&test_support(cells,actual)&&
        test_input_interaction()&&test_player_interaction(cells,actual)&&test_structure_interaction(cells,actual)&&test_geometry_cache(cells,actual);
    free(cells); free(actual);
    vg_close();
    return ok?0:1;
}
