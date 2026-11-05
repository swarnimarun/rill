// SDL GPU transport. Scene construction, matrices and verification live in Rill.
#include "renderer.h"
#include <SDL3/SDL.h>
#include <float.h>
#include <math.h>
#include <stddef.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>

enum { MAX_MESHES = 8, MAX_VERTICES = 4096 };
typedef struct Vertex { float position[3], normal[3]; } Vertex;
typedef struct FrameUniform { float mvp[16], model[16]; } FrameUniform;
typedef struct MaterialUniform { float color[4], light[4]; } MaterialUniform;
typedef struct Mesh { Vertex *vertices; SDL_GPUBuffer *buffer; uint32_t used, capacity; } Mesh;
_Static_assert(sizeof(float) == 4 && FLT_RADIX == 2 && FLT_MANT_DIG == 24, "f32 host contract");
_Static_assert(sizeof(Vertex) == 24 && offsetof(Vertex, normal) == 12, "vertex contract");
_Static_assert(sizeof(FrameUniform) == 128 && offsetof(FrameUniform, model) == 64, "matrix contract");
_Static_assert(sizeof(MaterialUniform) == 32 && offsetof(MaterialUniform, light) == 16, "material contract");

struct Engine {
    SDL_GPUDevice *device;
    SDL_Window *window;
    SDL_GPUGraphicsPipeline *pipeline;
    SDL_GPUTexture *color, *depth, *swapchain;
    SDL_GPUCommandBuffer *command;
    SDL_GPURenderPass *pass;
    Mesh meshes[MAX_MESHES];
    FrameUniform frame;
    MaterialUniform material;
    unsigned row_mask;
    bool material_ready, headless, quit, image_dirty;
    uint32_t width, height, frames, draws, submitted_vertices;
    int32_t limit, mode, mouse[2];
    Uint64 last_tick;
    unsigned char *pixels;
    size_t pixel_capacity;
    char assets[4096];
};
static int32_t last_status;

static bool fail(const char *operation) {
    last_status = 1;
    fprintf(stderr, "Engine %s: %s\n", operation, SDL_GetError());
    return false;
}
static bool invalid(const char *message) {
    SDL_SetError("%s", message);
    return fail("protocol");
}
static bool pixel_bytes(uint32_t width, uint32_t height, size_t *bytes) {
    if (width == 0 || height == 0) return invalid("render dimensions must be positive");
    size_t w = width, h = height;
    if (w > SIZE_MAX / h || w * h > SIZE_MAX / 4)
        return invalid("render image exceeds the host allocation size range");
    size_t size = w * h * 4;
    if (size > UINT32_MAX)
        return invalid("render image exceeds the 32-bit GPU transfer size limit");
    *bytes = size;
    return true;
}
static int32_t setting(const char *name, int32_t fallback, int32_t low, int32_t high) {
    const char *text = getenv(name);
    if (!text || !*text) return fallback;
    char *end = NULL;
    long value = strtol(text, &end, 10);
    if (*end || value < low || value > high) {
        fprintf(stderr, "%s must be between %d and %d\n", name, low, high);
        last_status = 1;
        return -1;
    }
    return (int32_t)value;
}
static bool enabled(const char *name) {
    const char *value = getenv(name);
    return value && strcmp(value, "1") == 0;
}
static bool submit_wait(Engine *engine, SDL_GPUCommandBuffer *command) {
    SDL_GPUFence *fence = SDL_SubmitGPUCommandBufferAndAcquireFence(command);
    if (!fence) return fail("submit fence");
    bool ok = SDL_WaitForGPUFences(engine->device, true, &fence, 1);
    SDL_ReleaseGPUFence(engine->device, fence);
    return ok || fail("wait fence");
}
static SDL_GPUShader *load_shader(Engine *engine, bool vertex) {
    bool metal = (SDL_GetGPUShaderFormats(engine->device) & SDL_GPU_SHADERFORMAT_MSL) != 0;
    const char *name = vertex ? (metal ? "mesh_vertex_msl.metal" : "mesh_vertex_spv.spv")
                              : (metal ? "mesh_fragment_msl.metal" : "mesh_fragment_spv.spv");
    char path[8192];
    int count = snprintf(path, sizeof path, "%s/%s", engine->assets, name);
    if (count < 0 || (size_t)count >= sizeof path) { invalid("shader path too long"); return NULL; }
    size_t bytes = 0;
    void *code = SDL_LoadFile(path, &bytes);
    if (!code) { fail(path); return NULL; }
    SDL_GPUShaderCreateInfo info = {
        .code_size = bytes, .code = code,
        .entrypoint = vertex ? "vertex_main" : "fragment_main",
        .format = metal ? SDL_GPU_SHADERFORMAT_MSL : SDL_GPU_SHADERFORMAT_SPIRV,
        .stage = vertex ? SDL_GPU_SHADERSTAGE_VERTEX : SDL_GPU_SHADERSTAGE_FRAGMENT,
        .num_uniform_buffers = 1
    };
    SDL_GPUShader *shader = SDL_CreateGPUShader(engine->device, &info);
    SDL_free(code);
    if (!shader) fail(vertex ? "vertex shader" : "fragment shader");
    return shader;
}
static bool pipeline(Engine *engine) {
    SDL_GPUShader *vertex = load_shader(engine, true);
    if (!vertex) return false;
    SDL_GPUShader *fragment = load_shader(engine, false);
    if (!fragment) { SDL_ReleaseGPUShader(engine->device, vertex); return false; }
    SDL_GPUVertexBufferDescription buffer = { .slot = 0, .pitch = sizeof(Vertex), .input_rate = SDL_GPU_VERTEXINPUTRATE_VERTEX };
    SDL_GPUVertexAttribute attributes[2] = {
        { .location = 0, .buffer_slot = 0, .format = SDL_GPU_VERTEXELEMENTFORMAT_FLOAT3, .offset = 0 },
        { .location = 1, .buffer_slot = 0, .format = SDL_GPU_VERTEXELEMENTFORMAT_FLOAT3, .offset = 12 }
    };
    SDL_GPUColorTargetDescription color = { .format = SDL_GPU_TEXTUREFORMAT_R8G8B8A8_UNORM };
    color.blend_state.color_write_mask = SDL_GPU_COLORCOMPONENT_R | SDL_GPU_COLORCOMPONENT_G | SDL_GPU_COLORCOMPONENT_B | SDL_GPU_COLORCOMPONENT_A;
    color.blend_state.enable_color_write_mask = true;
    SDL_GPUGraphicsPipelineCreateInfo info = {
        .vertex_shader = vertex, .fragment_shader = fragment,
        .vertex_input_state = { .vertex_buffer_descriptions = &buffer, .num_vertex_buffers = 1,
            .vertex_attributes = attributes, .num_vertex_attributes = 2 },
        .primitive_type = SDL_GPU_PRIMITIVETYPE_TRIANGLELIST,
        .rasterizer_state = { .fill_mode = SDL_GPU_FILLMODE_FILL, .cull_mode = SDL_GPU_CULLMODE_NONE,
            .front_face = SDL_GPU_FRONTFACE_COUNTER_CLOCKWISE },
        .multisample_state = { .sample_count = SDL_GPU_SAMPLECOUNT_1 },
        .depth_stencil_state = { .compare_op = SDL_GPU_COMPAREOP_LESS, .enable_depth_test = true, .enable_depth_write = true },
        .target_info = { .color_target_descriptions = &color, .num_color_targets = 1,
            .depth_stencil_format = SDL_GPU_TEXTUREFORMAT_D32_FLOAT, .has_depth_stencil_target = true }
    };
    engine->pipeline = SDL_CreateGPUGraphicsPipeline(engine->device, &info);
    SDL_ReleaseGPUShader(engine->device, fragment);
    SDL_ReleaseGPUShader(engine->device, vertex);
    return engine->pipeline != NULL || fail("graphics pipeline");
}
static bool targets(Engine *engine, uint32_t width, uint32_t height) {
    size_t bytes = 0;
    if (!pixel_bytes(width, height, &bytes)) return false;
    if (engine->color && width == engine->width && height == engine->height) return true;
    if (!SDL_WaitForGPUIdle(engine->device)) return fail("resize wait");
    if (engine->color) SDL_ReleaseGPUTexture(engine->device, engine->color);
    if (engine->depth) SDL_ReleaseGPUTexture(engine->device, engine->depth);
    engine->color = engine->depth = NULL;
    engine->width = width; engine->height = height;
    SDL_GPUTextureCreateInfo color = { .type = SDL_GPU_TEXTURETYPE_2D,
        .format = SDL_GPU_TEXTUREFORMAT_R8G8B8A8_UNORM,
        .usage = SDL_GPU_TEXTUREUSAGE_COLOR_TARGET | SDL_GPU_TEXTUREUSAGE_SAMPLER,
        .width = width, .height = height, .layer_count_or_depth = 1,
        .num_levels = 1, .sample_count = SDL_GPU_SAMPLECOUNT_1 };
    engine->color = SDL_CreateGPUTexture(engine->device, &color);
    color.format = SDL_GPU_TEXTUREFORMAT_D32_FLOAT;
    color.usage = SDL_GPU_TEXTUREUSAGE_DEPTH_STENCIL_TARGET;
    engine->depth = SDL_CreateGPUTexture(engine->device, &color);
    return (engine->color && engine->depth) || fail("render targets");
}

Engine *e3_open(int32_t width, int32_t height) {
    last_status = 0;
    int32_t chosen_width = setting("RILL_ENGINE_WIDTH", width, 32, 2048);
    int32_t chosen_height = setting("RILL_ENGINE_HEIGHT", height, 32, 2048);
    int32_t mode = setting("RILL_ENGINE_MODE", 0, 0, 2);
    bool headless = enabled("RILL_ENGINE_HEADLESS");
    int32_t limit = setting("RILL_ENGINE_FRAMES", headless ? 4 : 0, 0, 10000);
    if (chosen_width < 32 || chosen_width > 2048 || chosen_height < 32 || chosen_height > 2048 || mode < 0 || limit < 0) {
        invalid("invalid renderer configuration"); return NULL;
    }
    if (headless && limit == 0) {
        invalid("headless rendering requires a positive frame budget"); return NULL;
    }
    Engine *engine = calloc(1, sizeof *engine);
    if (!engine) { invalid("cannot allocate renderer"); return NULL; }
    engine->headless = headless; engine->limit = limit; engine->mode = mode;
    const char *assets = getenv("RILL_ENGINE_SHADERS");
    if (!assets || !*assets) assets = getenv("RILL_PROJECT_SHADER_DIR");
    const char *base = SDL_GetBasePath();
    int count = assets ? snprintf(engine->assets, sizeof engine->assets, "%s", assets)
        : snprintf(engine->assets, sizeof engine->assets, "%sshaders", base ? base : "");
    if (count < 0 || (size_t)count >= sizeof engine->assets) {
        invalid("shader directory too long"); e3_close(engine); return NULL;
    }
    if (!SDL_Init(SDL_INIT_VIDEO)) {
        fprintf(stderr, "Engine GPU unavailable: %s\n", SDL_GetError());
        last_status = 77; e3_close(engine); return NULL;
    }
    engine->device = SDL_CreateGPUDevice(SDL_GPU_SHADERFORMAT_MSL | SDL_GPU_SHADERFORMAT_SPIRV, true, NULL);
    if (!engine->device) {
        fprintf(stderr, "Engine GPU unavailable: %s\n", SDL_GetError());
        last_status = 77; e3_close(engine); return NULL;
    }
    if (!headless) {
        engine->window = SDL_CreateWindow("Rill 3D | WASD move, Q/E vertical, drag to look, Esc quit",
            chosen_width, chosen_height, SDL_WINDOW_RESIZABLE);
        if (!engine->window || !SDL_ClaimWindowForGPUDevice(engine->device, engine->window)) {
            fail("claim window"); e3_close(engine); return NULL;
        }
    }
    if (!targets(engine, (uint32_t)chosen_width, (uint32_t)chosen_height) || !pipeline(engine)) {
        e3_close(engine); return NULL;
    }
    engine->last_tick = SDL_GetTicks();
    fprintf(stderr, "Engine backend=%s validation=enabled mode=%s\n", SDL_GetGPUDeviceDriver(engine->device), headless ? "offscreen" : "window");
    return engine;
}
void e3_close(Engine *engine) {
    if (!engine) return;
    if (engine->pass) { SDL_EndGPURenderPass(engine->pass); engine->pass = NULL; }
    if (engine->command) { SDL_SubmitGPUCommandBuffer(engine->command); engine->command = NULL; }
    if (engine->device) {
        SDL_WaitForGPUIdle(engine->device);
        fprintf(stdout, "ENGINE frames=%u draws=%u vertices=%u size=%ux%u backend=%s\n",
            engine->frames, engine->draws, engine->submitted_vertices, engine->width, engine->height,
            SDL_GetGPUDeviceDriver(engine->device));
        for (unsigned i = 0; i < MAX_MESHES; ++i) {
            if (engine->meshes[i].buffer) SDL_ReleaseGPUBuffer(engine->device, engine->meshes[i].buffer);
            free(engine->meshes[i].vertices);
        }
        if (engine->pipeline) SDL_ReleaseGPUGraphicsPipeline(engine->device, engine->pipeline);
        if (engine->color) SDL_ReleaseGPUTexture(engine->device, engine->color);
        if (engine->depth) SDL_ReleaseGPUTexture(engine->device, engine->depth);
        if (engine->window) SDL_ReleaseWindowFromGPUDevice(engine->device, engine->window);
        SDL_DestroyGPUDevice(engine->device);
    }
    if (engine->window) SDL_DestroyWindow(engine->window);
    free(engine->pixels); free(engine); SDL_Quit();
}
int32_t e3_status(void) { return last_status; }
bool e3_headless(Engine *engine) { return engine && engine->headless; }
int32_t e3_mode(Engine *engine) { return engine ? engine->mode : 0; }
int32_t e3_limit(Engine *engine) { return engine ? engine->limit : 0; }
int32_t e3_width(Engine *engine) { return engine ? (int32_t)engine->width : 0; }
int32_t e3_height(Engine *engine) { return engine ? (int32_t)engine->height : 0; }
bool e3_poll(Engine *engine) {
    if (!engine) return invalid("poll without renderer");
    engine->mouse[0] = engine->mouse[1] = 0;
    SDL_Event event;
    while (SDL_PollEvent(&event)) {
        if (event.type == SDL_EVENT_QUIT) engine->quit = true;
        if (!engine->headless && event.type == SDL_EVENT_MOUSE_MOTION && (event.motion.state & SDL_BUTTON_LMASK)) {
            float x = event.motion.xrel, y = event.motion.yrel;
            if (isfinite(x) && isfinite(y)) {
                engine->mouse[0] = (int32_t)fmaxf(-1000, fminf(1000, (float)engine->mouse[0] + x));
                engine->mouse[1] = (int32_t)fmaxf(-1000, fminf(1000, (float)engine->mouse[1] + y));
            }
        }
    }
    return !engine->quit;
}
int32_t e3_key(Engine *engine, int32_t action) {
    static const SDL_Scancode keys[] = { SDL_SCANCODE_W, SDL_SCANCODE_S, SDL_SCANCODE_A, SDL_SCANCODE_D,
        SDL_SCANCODE_Q, SDL_SCANCODE_E, SDL_SCANCODE_LEFT, SDL_SCANCODE_RIGHT, SDL_SCANCODE_UP, SDL_SCANCODE_DOWN, SDL_SCANCODE_ESCAPE };
    if (!engine || engine->headless || action < 0 || (size_t)action >= sizeof keys / sizeof keys[0]) return 0;
    return SDL_GetKeyboardState(NULL)[keys[action]] ? 1 : 0;
}
int32_t e3_mouse(Engine *engine, int32_t axis) { return engine && axis >= 0 && axis < 2 ? engine->mouse[axis] : 0; }
int32_t e3_elapsed(Engine *engine) {
    if (!engine || engine->headless) return 16;
    Uint64 now = SDL_GetTicks(), elapsed = now - engine->last_tick;
    engine->last_tick = now;
    return elapsed > 100 ? 100 : elapsed < 1 ? 1 : (int32_t)elapsed;
}
float e3_sin(int32_t angle) { return sinf((float)angle * (3.14159265358979323846f / 180000.0f)); }
float e3_cos(int32_t angle) { return cosf((float)angle * (3.14159265358979323846f / 180000.0f)); }

int32_t e3_mesh_new(Engine *engine, int32_t capacity) {
    if (!engine || capacity < 1 || capacity > MAX_VERTICES) { invalid("invalid mesh capacity"); return -1; }
    for (unsigned i = 0; i < MAX_MESHES; ++i) {
        Mesh *mesh = &engine->meshes[i];
        if (!mesh->vertices && !mesh->buffer) {
            mesh->vertices = calloc((size_t)capacity, sizeof(Vertex));
            if (!mesh->vertices) { invalid("cannot allocate mesh staging"); return -1; }
            mesh->capacity = (uint32_t)capacity; return (int32_t)i;
        }
    }
    invalid("mesh capacity exhausted"); return -1;
}
bool e3_vertex(Engine *engine, int32_t index, float x, float y, float z, float nx, float ny, float nz) {
    if (!engine || index < 0 || index >= MAX_MESHES) return invalid("unknown mesh");
    Mesh *mesh = &engine->meshes[index];
    if (!mesh->vertices || mesh->buffer || mesh->used >= mesh->capacity) return invalid("mesh is sealed or full");
    if (!isfinite(x) || !isfinite(y) || !isfinite(z) || !isfinite(nx) || !isfinite(ny) || !isfinite(nz)
        || (nx == 0 && ny == 0 && nz == 0)) return invalid("vertices require finite values and a nonzero normal");
    mesh->vertices[mesh->used++] = (Vertex){ { x, y, z }, { nx, ny, nz } };
    return true;
}
bool e3_mesh_upload(Engine *engine, int32_t index) {
    if (!engine || index < 0 || index >= MAX_MESHES) return invalid("unknown mesh");
    Mesh *mesh = &engine->meshes[index];
    if (!mesh->vertices || mesh->buffer || mesh->used == 0 || mesh->used % 3) return invalid("mesh requires complete triangles");
    uint32_t bytes = mesh->used * (uint32_t)sizeof(Vertex);
    SDL_GPUBufferCreateInfo bi = { .usage = SDL_GPU_BUFFERUSAGE_VERTEX, .size = bytes };
    mesh->buffer = SDL_CreateGPUBuffer(engine->device, &bi);
    if (!mesh->buffer) return fail("vertex buffer");
    SDL_GPUTransferBufferCreateInfo ti = { .usage = SDL_GPU_TRANSFERBUFFERUSAGE_UPLOAD, .size = bytes };
    SDL_GPUTransferBuffer *transfer = SDL_CreateGPUTransferBuffer(engine->device, &ti);
    if (!transfer) return fail("vertex transfer");
    void *mapped = SDL_MapGPUTransferBuffer(engine->device, transfer, false);
    if (!mapped) { SDL_ReleaseGPUTransferBuffer(engine->device, transfer); return fail("vertex map"); }
    memcpy(mapped, mesh->vertices, bytes); SDL_UnmapGPUTransferBuffer(engine->device, transfer);
    SDL_GPUCommandBuffer *command = SDL_AcquireGPUCommandBuffer(engine->device);
    if (!command) { SDL_ReleaseGPUTransferBuffer(engine->device, transfer); return fail("vertex command"); }
    SDL_GPUCopyPass *copy = SDL_BeginGPUCopyPass(command);
    SDL_GPUTransferBufferLocation source = { .transfer_buffer = transfer };
    SDL_GPUBufferRegion destination = { .buffer = mesh->buffer, .size = bytes };
    SDL_UploadToGPUBuffer(copy, &source, &destination, false); SDL_EndGPUCopyPass(copy);
    bool ok = submit_wait(engine, command);
    SDL_ReleaseGPUTransferBuffer(engine->device, transfer);
    if (ok) { free(mesh->vertices); mesh->vertices = NULL; }
    return ok;
}
bool e3_begin(Engine *engine) {
    if (!engine || engine->command || engine->pass) return invalid("frame already active");
    engine->command = SDL_AcquireGPUCommandBuffer(engine->device);
    if (!engine->command) return fail("frame command");
    engine->swapchain = NULL;
    if (engine->window) {
        uint32_t width = 0, height = 0;
        if (!SDL_WaitAndAcquireGPUSwapchainTexture(engine->command, engine->window, &engine->swapchain, &width, &height)) return fail("acquire swapchain");
        if (engine->swapchain && !targets(engine, width, height)) return false;
    }
    SDL_GPUColorTargetInfo color = { .texture = engine->color,
        .clear_color = { 0.02f, 0.03f, 0.05f, 1 }, .load_op = SDL_GPU_LOADOP_CLEAR,
        .store_op = SDL_GPU_STOREOP_STORE, .cycle = true };
    SDL_GPUDepthStencilTargetInfo depth = { .texture = engine->depth,
        .clear_depth = 1, .load_op = SDL_GPU_LOADOP_CLEAR, .store_op = SDL_GPU_STOREOP_DONT_CARE,
        .stencil_load_op = SDL_GPU_LOADOP_DONT_CARE, .stencil_store_op = SDL_GPU_STOREOP_DONT_CARE,
        .cycle = true };
    engine->pass = SDL_BeginGPURenderPass(engine->command, &color, 1, &depth);
    if (!engine->pass) return fail("render pass");
    SDL_BindGPUGraphicsPipeline(engine->pass, engine->pipeline);
    SDL_GPUViewport viewport = { 0, 0, (float)engine->width, (float)engine->height, 0, 1 };
    SDL_SetGPUViewport(engine->pass, &viewport);
    engine->row_mask = 0; engine->material_ready = false;
    return true;
}
bool e3_matrix(Engine *engine, int32_t slot, int32_t row, float x, float y, float z, float w) {
    if (!engine || !engine->pass || slot < 0 || slot > 1 || row < 0 || row > 3) return invalid("invalid matrix row");
    if (!isfinite(x) || !isfinite(y) || !isfinite(z) || !isfinite(w)) return invalid("matrix requires finite values");
    float *matrix = slot == 0 ? engine->frame.mvp : engine->frame.model;
    matrix[row] = x; matrix[4 + row] = y; matrix[8 + row] = z; matrix[12 + row] = w;
    engine->row_mask |= 1u << (slot * 4 + row); return true;
}
bool e3_material(Engine *engine, float r, float g, float b, float lx, float ly, float lz, float ambient) {
    if (!engine || !engine->pass) return invalid("material outside frame");
    if (!isfinite(r) || !isfinite(g) || !isfinite(b) || !isfinite(lx) || !isfinite(ly) || !isfinite(lz)
        || !isfinite(ambient) || ambient < 0 || ambient > 1 || (lx == 0 && ly == 0 && lz == 0)) return invalid("invalid material or light");
    engine->material = (MaterialUniform){ { r, g, b, 1 }, { lx, ly, lz, ambient } };
    engine->material_ready = true; return true;
}
bool e3_draw(Engine *engine, int32_t index) {
    if (!engine || !engine->pass || index < 0 || index >= MAX_MESHES || !engine->meshes[index].buffer
        || engine->row_mask != 255 || !engine->material_ready) return invalid("draw requires a mesh, two matrices and material");
    SDL_GPUBufferBinding binding = { .buffer = engine->meshes[index].buffer };
    SDL_BindGPUVertexBuffers(engine->pass, 0, &binding, 1);
    SDL_PushGPUVertexUniformData(engine->command, 0, &engine->frame, sizeof engine->frame);
    SDL_PushGPUFragmentUniformData(engine->command, 0, &engine->material, sizeof engine->material);
    SDL_DrawGPUPrimitives(engine->pass, engine->meshes[index].used, 1, 0, 0);
    engine->draws++; engine->submitted_vertices += engine->meshes[index].used;
    engine->row_mask = 0; engine->material_ready = false; return true;
}
bool e3_end(Engine *engine) {
    if (!engine || !engine->pass || !engine->command) return invalid("no active frame");
    SDL_EndGPURenderPass(engine->pass); engine->pass = NULL;
    if (engine->swapchain) {
        SDL_GPUBlitInfo blit = { .source = { .texture = engine->color, .w = engine->width, .h = engine->height },
            .destination = { .texture = engine->swapchain, .w = engine->width, .h = engine->height },
            .load_op = SDL_GPU_LOADOP_DONT_CARE, .filter = SDL_GPU_FILTER_NEAREST };
        SDL_BlitGPUTexture(engine->command, &blit);
    }
    SDL_GPUCommandBuffer *command = engine->command; engine->command = NULL;
    bool ok = engine->headless ? submit_wait(engine, command) : SDL_SubmitGPUCommandBuffer(command);
    if (!ok) return fail("frame submit");
    engine->frames++; engine->image_dirty = true;
    return last_status == 0;
}
static bool read_pixels(Engine *engine) {
    if (!engine || engine->command || engine->frames == 0) return invalid("readback requires a submitted frame");
    size_t bytes = 0;
    if (!pixel_bytes(engine->width, engine->height, &bytes)) return false;
    if (!engine->image_dirty && engine->pixels) return true;
    if (engine->pixel_capacity < bytes) {
        unsigned char *pixels = realloc(engine->pixels, bytes);
        if (!pixels) return invalid("cannot allocate readback pixels");
        engine->pixels = pixels; engine->pixel_capacity = bytes;
    }
    SDL_GPUTransferBufferCreateInfo ti = { .usage = SDL_GPU_TRANSFERBUFFERUSAGE_DOWNLOAD, .size = (uint32_t)bytes };
    SDL_GPUTransferBuffer *transfer = SDL_CreateGPUTransferBuffer(engine->device, &ti);
    if (!transfer) return fail("readback transfer");
    SDL_GPUCommandBuffer *command = SDL_AcquireGPUCommandBuffer(engine->device);
    if (!command) { SDL_ReleaseGPUTransferBuffer(engine->device, transfer); return fail("readback command"); }
    SDL_GPUCopyPass *copy = SDL_BeginGPUCopyPass(command);
    SDL_GPUTextureRegion source = { .texture = engine->color, .w = engine->width, .h = engine->height, .d = 1 };
    SDL_GPUTextureTransferInfo destination = { .transfer_buffer = transfer };
    SDL_DownloadFromGPUTexture(copy, &source, &destination); SDL_EndGPUCopyPass(copy);
    bool ok = submit_wait(engine, command);
    if (ok) {
        const void *mapped = SDL_MapGPUTransferBuffer(engine->device, transfer, false);
        if (mapped) { memcpy(engine->pixels, mapped, bytes); SDL_UnmapGPUTransferBuffer(engine->device, transfer); }
        else ok = fail("readback map");
    }
    SDL_ReleaseGPUTransferBuffer(engine->device, transfer);
    if (ok) engine->image_dirty = false;
    return ok;
}
int32_t e3_pixel(Engine *engine, int32_t x, int32_t y, int32_t component) {
    if (!engine || x < 0 || y < 0 || (uint32_t)x >= engine->width || (uint32_t)y >= engine->height
        || component < 0 || component > 3) { invalid("pixel out of range"); return -1; }
    return read_pixels(engine) ? engine->pixels[((size_t)y * engine->width + (uint32_t)x) * 4 + (uint32_t)component] : -1;
}
bool e3_capture(Engine *engine) {
    const char *path = getenv("RILL_ENGINE_CAPTURE");
    if (!path || !*path) return true;
    if (!read_pixels(engine)) return false;
    FILE *file = fopen(path, "wb");
    if (!file) return invalid("cannot open capture file");
    bool ok = fprintf(file, "P6\n%u %u\n255\n", engine->width, engine->height) > 0;
    for (size_t pixel = 0; ok && pixel < (size_t)engine->width * engine->height; ++pixel)
        ok = fwrite(engine->pixels + pixel * 4, 1, 3, file) == 3;
    if (fclose(file) != 0) ok = false;
    return ok || invalid("cannot write capture file");
}
