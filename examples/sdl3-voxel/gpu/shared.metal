#include <metal_stdlib>
using namespace metal;

// Matches GPUParams in gpu.h. Every member is sixteen bytes.
struct Params {
    float4 eye;
    float4 forward;
    float4 right;
    float4 up;
    float4 edit;
    float4 dynamics;
    uint4 world;
    uint4 frame;
    uint4 limits;
    uint4 config;
    float4 movement;
    uint4 controller;
};
struct Body {
    float4 position;
    float4 velocity;
    float4 angle;
    float4 spin;
    uint4 info;
};
// Explicit aligned tails in the existing body buffer keep SDL's eight-write-buffer limit.
struct Structure {
    float4 rest;
    float4 position;
    float4 velocity;
    float4 angle;
    float4 spin;
    float4 bounds_min;
    float4 bounds_max;
    uint4 info;
};
struct Player {
    float4 feet_ground;
    float4 velocity_height;
    float4 eye_mode;
    uint4 flags;
};
inline device Structure* structures(device Body* bodies, constant Params& p) {
    return reinterpret_cast<device Structure*>(bodies + p.limits.x);
}
inline device Player* player_state(device Body* bodies, constant Params& p) {
    return reinterpret_cast<device Player*>(structures(bodies,p) + 9);
}

// All kernels bind the same persistent storage. Dependent dispatches use
// separate SDL compute passes. The second half of parents holds dynamic voxels.
#define RESOURCES constant Params& p [[buffer(0)]], \
 device uint* cells [[buffer(1)]], device atomic_uint* parents [[buffer(2)]], \
 device uint* field [[buffer(3)]], device uint* bricks [[buffer(4)]], \
 device Body* bodies [[buffer(5)]], device uint* atlas [[buffer(6)]], \
 device float4* probes [[buffer(7)]], device atomic_uint* stats [[buffer(8)]]

inline uint volume(constant Params& p) { return p.world.w; }
inline bool inside(int3 c, constant Params& p) { return all(c >= int3(0)) && all(c < int3(p.world.xyz)); }
inline uint index3(uint3 c, uint n) { return c.x + n * (c.y + n * c.z); }
inline uint3 coordinate(uint i, uint n) { return uint3(i % n, (i / n) % n, i / (n*n)); }
inline uint material(int3 c, constant Params& p, device uint* cells, device atomic_uint* parents) {
    if (!inside(c,p)) return 0;
    uint i = index3(uint3(c),p.world.x);
    uint solid = cells[i];
    return solid != 0 ? solid : atomic_load_explicit(parents + p.world.w + 1 + i, memory_order_relaxed);
}
inline float3 rotate_xyz(float3 v, float3 a) {
    float3 s = sin(a), c = cos(a);
    v = float3(v.x, c.x*v.y-s.x*v.z, s.x*v.y+c.x*v.z);
    v = float3(c.y*v.x+s.y*v.z, v.y, -s.y*v.x+c.y*v.z);
    return float3(c.z*v.x-s.z*v.y, s.z*v.x+c.z*v.y, v.z);
}
inline uint hash32(uint x) { x ^= x>>16; x *= 0x7feb352du; x ^= x>>15; x *= 0x846ca68bu; return x ^ (x>>16); }
inline float random01(uint x) { return float(hash32(x) & 0xffffffu) / 16777216.0f; }
