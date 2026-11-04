// GPU-resident materials, connected support and 4-cubed rigid fragments.
// shared.metal is prepended by the host before pipeline compilation.

// Pure reference fixtures leave structural mode disabled and rebuild on every
// requested pass. The city retains its geometry until an actual edit or pose
// change invalidates it. The host dispatches frame_begin before frame work.
inline bool geometry_unchanged(device atomic_uint* stats) {
    return atomic_load_explicit(stats+12,memory_order_relaxed)!=0u &&
        atomic_load_explicit(stats+17,memory_order_relaxed)==0u;
}
inline bool distance_unchanged(device atomic_uint* stats) {
    return atomic_load_explicit(stats+12,memory_order_relaxed)!=0u &&
        atomic_load_explicit(stats+21,memory_order_relaxed)==0u;
}
inline bool materials_unchanged(device atomic_uint* stats) {
    return atomic_load_explicit(stats+12,memory_order_relaxed)!=0u &&
        atomic_load_explicit(stats+18,memory_order_relaxed)==0u;
}
// Material edits are classified independently of pose changes: static world
// 1, building rest atlas 2, loose-body atlas 4. OR preserves simultaneous edits.
inline bool static_materials_unchanged(device atomic_uint* stats) {
    return atomic_load_explicit(stats+12,memory_order_relaxed)!=0u &&
        (atomic_load_explicit(stats+18,memory_order_relaxed)&1u)==0u;
}
inline bool structure_materials_unchanged(device atomic_uint* stats) {
    return atomic_load_explicit(stats+12,memory_order_relaxed)!=0u &&
        (atomic_load_explicit(stats+18,memory_order_relaxed)&3u)==0u;
}
kernel void frame_begin(RESOURCES,uint id [[thread_position_in_grid]]) {
    if(id==0u) {
        atomic_store_explicit(stats+17,0u,memory_order_relaxed);
        atomic_store_explicit(stats+18,0u,memory_order_relaxed);
    }
}

inline float4 brush(constant Params& p, device atomic_uint* stats) {
    if ((p.frame.w & 1u) == 0u) return p.edit;
    if (atomic_load_explicit(stats + 11, memory_order_relaxed) == 0u) return float4(0);
    return float4(as_type<float>(atomic_load_explicit(stats + 8, memory_order_relaxed)),
                  as_type<float>(atomic_load_explicit(stats + 9, memory_order_relaxed)),
                  as_type<float>(atomic_load_explicit(stats + 10, memory_order_relaxed)), p.edit.w);
}

inline uint architecture(uint3 c, uint n) {
    if (c.y == 0u) return 1u;
    uint tile = n / 3u, margin = max(3u, n / 32u);
    uint thick = max(1u, n / 64u), story = max(5u, n / 12u);
    uint2 block = c.xz / tile;
    if (any(block >= uint2(3u))) return 0u;
    uint2 local = c.xz % tile;
    uint low = margin + 1u, high = tile - margin - 1u;
    if (any(local < uint2(low)) || any(local >= uint2(high))) return 0u;
    uint base = story, top = base + n / 3u + (hash32(block.x + 3u*block.y) % 3u) * story;
    uint middle = tile / 2u, half_column = max(2u, n / 64u);
    if (c.y < base) {
        return all(abs(int2(local) - int2(middle)) < int2(half_column)) ? 5u : 0u;
    }
    if (c.y >= top) return 0u;
    uint phase = (c.y - base) % story;
    bool edge_x = local.x < low + thick || local.x >= high - thick;
    bool edge_z = local.y < low + thick || local.y >= high - thick;
    bool floor = phase < thick || c.y >= top - thick;
    if (floor) {
        if (edge_x || edge_z) return 5u;
        return ((c.y - base) / story) % 2u == 0u ? 6u : 3u;
    }
    // The lights hang under slabs and remain actual destructible emitters.
    if (phase == story - 1u && local.x % max(3u,n/24u) == 0u && local.y % max(3u,n/24u) == 0u) return 4u;
    if (edge_x || edge_z) {
        uint along = edge_x ? local.y : local.x;
        uint pitch = max(3u, n / 24u);
        bool opening = phase > thick + 1u && phase < story - 2u && along % pitch > thick;
        if (opening) return along % pitch == thick + 1u ? 7u : 0u;
        return (edge_x && edge_z) || along % pitch < thick ? 5u : 2u;
    }
    // Interior columns connect successive floors without filling the rooms.
    bool column_x = local.x == low + 2u*thick || local.x == high - 2u*thick;
    bool column_z = local.y == low + 2u*thick || local.y == high - 2u*thick;
    return column_x && column_z ? 5u : 0u;
}

kernel void reset_world(RESOURCES, uint id [[thread_position_in_grid]]) {
    if (id >= volume(p)) return;
    uint value = architecture(coordinate(id,p.world.x),p.world.x);
    cells[id] = value;
    cells[volume(p)+id] = 0u;
    field[id] = 0u;
    atomic_store_explicit(parents + id + 1u,id + 1u,memory_order_relaxed);
    atomic_store_explicit(parents + volume(p) + 1u + id,0u,memory_order_relaxed);
    if (id == 0u) {
        atomic_store_explicit(parents,0u,memory_order_relaxed);
        atomic_store_explicit(stats+17,1u,memory_order_relaxed);
        atomic_store_explicit(stats+21,1u,memory_order_relaxed);
        atomic_fetch_or_explicit(stats+18,1u,memory_order_relaxed);
    }
    if (value != 0u) atomic_fetch_add_explicit(stats + 5,1u,memory_order_relaxed);
    if (id < p.limits.x) {
        Body body;
        body.position = body.velocity = body.angle = body.spin = float4(0);
        body.info = uint4(0);
        bodies[id] = body;
    }
    uint brick_count = p.config.x*p.config.x*p.config.x;
    if (id < brick_count) bricks[id] = 0u;
    // The host clears counters before this pass, and clears probes separately.
}

kernel void edit_world(RESOURCES, uint id [[thread_position_in_grid]]) {
    float4 edit = brush(p,stats);
    if (edit.w <= 0.0f) return;
    float radius2 = edit.w*edit.w;
    if (id < volume(p)) {
        if (cells[id] > 1u) {
            float3 delta = float3(coordinate(id,p.world.x)) + 0.5f - edit.xyz;
            if (dot(delta,delta) <= radius2) {
                cells[id] = 0u;
                atomic_fetch_add_explicit(stats + 1,1u,memory_order_relaxed);
                atomic_store_explicit(stats+17,1u,memory_order_relaxed);
                atomic_store_explicit(stats+21,1u,memory_order_relaxed);
                atomic_fetch_or_explicit(stats+18,1u,memory_order_relaxed);
            }
        }
        // Building voxels remain in rest coordinates while the assembly pose
        // moves. Editing tests their transformed centers, not the rest grid.
        if (atomic_load_explicit(stats+12,memory_order_relaxed) != 0u && cells[volume(p)+id] > 1u) {
            uint3 c = coordinate(id,p.world.x);
            uint tile = p.world.x/3u;
            uint building = c.x/tile + 3u*(c.z/tile);
            if (building < 9u) {
                Structure state = structures(bodies,p)[building];
                float3 position = state.position.xyz + rotate_xyz(float3(c)+0.5f-state.rest.xyz,state.angle.xyz);
                float3 delta = position-edit.xyz;
                if (dot(delta,delta) <= radius2) {
                    cells[volume(p)+id] = 0u;
                    atomic_fetch_add_explicit(stats+1,1u,memory_order_relaxed);
                    atomic_store_explicit(stats+17,1u,memory_order_relaxed);
                    atomic_store_explicit(stats+21,1u,memory_order_relaxed);
                    atomic_fetch_or_explicit(stats+18,2u,memory_order_relaxed);
                }
            }
        }
        return;
    }
    uint slot = id - volume(p);
    uint count = min(atomic_load_explicit(stats,memory_order_relaxed),p.limits.x);
    if (slot >= count) return;
    Body body = bodies[slot];
    if (body.info.y == 0u) return;
    uint removed = 0u;
    for (uint local = 0u; local < 64u; ++local) {
        uint value = atlas[slot*64u + local];
        if (value <= 1u) continue;
        float3 offset = float3(coordinate(local,4u)) + 0.5f - 2.0f;
        float3 delta = body.position.xyz + rotate_xyz(offset,body.angle.xyz) - edit.xyz;
        if (dot(delta,delta) <= radius2) {
            atlas[slot*64u + local] = 0u;
            ++removed;
        }
    }
    body.info.x -= min(body.info.x,removed);
    if (body.info.x == 0u) body.info.y = 0u;
    float3 delta = body.position.xyz - edit.xyz;
    float distance = length(delta);
    if (distance < edit.w + 6.0f) {
        body.info.w = body.info.z = 0u;
        float3 direction = distance > 0.001f ? delta/distance : float3(0,1,0);
        body.velocity.xyz += direction * p.dynamics.z / (1.0f + sqrt(float(max(body.info.x,1u))));
    }
    bodies[slot] = body;
    if (removed != 0u) {
        atomic_fetch_add_explicit(stats + 1,removed,memory_order_relaxed);
        atomic_store_explicit(stats+17,1u,memory_order_relaxed);
        atomic_store_explicit(stats+21,1u,memory_order_relaxed);
        atomic_fetch_or_explicit(stats+18,4u,memory_order_relaxed);
    }
}

kernel void support_seed(RESOURCES, uint id [[thread_position_in_grid]]) {
    if(static_materials_unchanged(stats)) return;
    if (id >= volume(p)) return;
    uint y = (id/p.world.x) % p.world.x;
    uint root = cells[id] == 1u || (cells[id] != 0u && y == 0u) ? 0u : id + 1u;
    atomic_store_explicit(parents + id + 1u,root,memory_order_relaxed);
    if (id == 0u) atomic_store_explicit(parents,0u,memory_order_relaxed);
}

inline uint root_of(uint node, device atomic_uint* parents, device atomic_uint* stats, uint bound) {
    for (uint step = 0u; step <= bound; ++step) {
        uint next = atomic_load_explicit(parents + node,memory_order_relaxed);
        if (next == node) return node;
        if (next > node || next > bound) break;
        uint after = atomic_load_explicit(parents + next,memory_order_relaxed);
        if (after > next) break;
        atomic_fetch_min_explicit(parents + node,after,memory_order_relaxed);
        node = next;
    }
    atomic_store_explicit(stats + 3,1u,memory_order_relaxed);
    return 0u;
}

inline void join_roots(uint a, uint b, device atomic_uint* parents, device atomic_uint* stats, uint bound) {
    for (uint attempt = 0u; attempt <= bound; ++attempt) {
        uint ra = root_of(a,parents,stats,bound), rb = root_of(b,parents,stats,bound);
        if (ra == rb) return;
        uint high = max(ra,rb), low = min(ra,rb), expected = high;
        if (atomic_compare_exchange_weak_explicit(parents + high,&expected,low,memory_order_relaxed,memory_order_relaxed)) return;
    }
    atomic_store_explicit(stats + 3,1u,memory_order_relaxed);
}

kernel void support_union(RESOURCES, uint id [[thread_position_in_grid]]) {
    if(static_materials_unchanged(stats)) return;
    if (id >= volume(p) || cells[id] == 0u) return;
    uint n = p.world.x;
    uint3 c = coordinate(id,n);
    if (c.x + 1u < n && cells[id + 1u] != 0u) join_roots(id+1u,id+2u,parents,stats,volume(p));
    if (c.y + 1u < n && cells[id + n] != 0u) join_roots(id+1u,id+n+1u,parents,stats,volume(p));
    if (c.z + 1u < n && cells[id + n*n] != 0u) join_roots(id+1u,id+n*n+1u,parents,stats,volume(p));
}

inline uint reserve_body(device atomic_uint* stats,uint capacity) {
    uint slot = atomic_load_explicit(stats,memory_order_relaxed);
    for (uint attempt = 0u; attempt <= capacity; ++attempt) {
        if (slot >= capacity) break;
        uint expected = slot;
        if (atomic_compare_exchange_weak_explicit(stats,&expected,slot+1u,memory_order_relaxed,memory_order_relaxed)) return slot;
        slot = expected;
    }
    atomic_fetch_add_explicit(stats+4,1u,memory_order_relaxed);
    return capacity;
}

kernel void detach_bodies(RESOURCES,uint id [[thread_position_in_grid]]) {
    if(static_materials_unchanged(stats)) return;
    uint chunks = p.world.x / 4u;
    if (id >= chunks*chunks*chunks || atomic_load_explicit(stats+3,memory_order_relaxed) != 0u) return;
    uint3 base = coordinate(id,chunks)*4u;
    uint values[64], mass = 0u;
    for (uint local = 0u; local < 64u; ++local) {
        uint index = index3(base+coordinate(local,4u),p.world.x);
        uint value = cells[index];
        values[local] = value > 1u && root_of(index+1u,parents,stats,volume(p)) != 0u ? value : 0u;
        if (values[local] != 0u) ++mass;
    }
    if (mass == 0u || atomic_load_explicit(stats+3,memory_order_relaxed) != 0u) return;
    uint slot = reserve_body(stats,p.limits.x);
    if (slot == p.limits.x) return; // Retain every static voxel on exhaustion.
    for (uint local = 0u; local < 64u; ++local) {
        atlas[slot*64u+local] = values[local];
        if (values[local] != 0u) cells[index3(base+coordinate(local,4u),p.world.x)] = 0u;
    }
    Body body;
    body.position = float4(float3(base)+2.0f,1);
    body.angle = float4(0);
    float4 edit = brush(p,stats);
    float3 delta = body.position.xyz-edit.xyz;
    float distance = length(delta);
    float3 direction = distance > 0.001f ? delta/distance : float3(0,1,0);
    float impulse = edit.w > 0.0f ? p.dynamics.z / (1.0f + distance/max(edit.w,1.0f)) : 0.0f;
    body.velocity = float4(direction*impulse/sqrt(float(mass)),0);
    body.spin = float4((float3(random01(id*3u),random01(id*3u+1u),random01(id*3u+2u))-0.5f)*impulse*0.05f,0);
    body.info = uint4(mass,1u,0u,0u);
    bodies[slot] = body;
    atomic_fetch_add_explicit(stats+2,mass,memory_order_relaxed);
    atomic_store_explicit(stats+17,1u,memory_order_relaxed);
    atomic_store_explicit(stats+21,1u,memory_order_relaxed);
    atomic_fetch_or_explicit(stats+18,1u,memory_order_relaxed);
}

inline bool body_collision(float3 position,float3 angle,uint slot,constant Params& p,device uint* cells,device uint* atlas) {
    float3 extent = (abs(rotate_xyz(float3(1,0,0),angle)) + abs(rotate_xyz(float3(0,1,0),angle)) + abs(rotate_xyz(float3(0,0,1),angle)))*0.495f;
    for (uint local = 0u; local < 64u; ++local) {
        if (atlas[slot*64u+local] == 0u) continue;
        float3 center = position + rotate_xyz(float3(coordinate(local,4u))+0.5f-2.0f,angle);
        int3 low = int3(floor(center-extent)), high = int3(floor(center+extent));
        if (low.y < 0 || low.x < 0 || low.z < 0 || high.x >= int(p.world.x) || high.z >= int(p.world.z)) return true;
        // Conservative transformed-voxel bounds may stop a fragment slightly
        // early. They never substitute disappearing particles for its mass.
        for (int z = low.z; z <= high.z; ++z) for (int y = low.y; y <= high.y; ++y) for (int x = low.x; x <= high.x; ++x) {
            int3 c(x,y,z);
            if (inside(c,p) && cells[index3(uint3(c),p.world.x)] != 0u) return true;
        }
    }
    return false;
}

kernel void integrate_bodies(RESOURCES,uint id [[thread_position_in_grid]]) {
    if (id == 0u) atomic_fetch_add_explicit(stats+6,1u,memory_order_relaxed);
    uint count = min(atomic_load_explicit(stats,memory_order_relaxed),p.limits.x);
    if (id >= count) return;
    Body body = bodies[id];
    if (body.info.y == 0u) return;
    float3 old_position=body.position.xyz,old_angle=body.angle.xyz;
    if (body.info.w != 0u) {
        // A distant edit can detach the static slab this body rested on.
        // Sleeping bodies must validate support against the current world.
        if (body_collision(body.position.xyz-float3(0,0.05f,0),body.angle.xyz,id,p,cells,atlas)) return;
        body.info.w = body.info.z = 0u;
    }
    float dt = clamp(p.dynamics.x,0.0f,0.0333334f);
    body.velocity.y -= max(p.dynamics.y,0.0f)*dt;
    body.velocity.xyz = clamp(body.velocity.xyz,float3(-40),float3(40));
    body.spin.xyz = clamp(body.spin.xyz,float3(-3),float3(3));
    uint steps = uint(clamp(ceil(length(body.velocity.xyz)*dt/0.25f),1.0f,10.0f));
    float subdt = dt/float(steps);
    bool floor_contact = false;
    for (uint step = 0u; step < steps; ++step) {
        float3 angle = body.angle.xyz + body.spin.xyz*subdt;
        if (!body_collision(body.position.xyz,angle,id,p,cells,atlas)) body.angle.xyz = angle;
        else body.spin.xyz *= -0.2f;
        for (uint axis = 0u; axis < 3u; ++axis) {
            float3 position = body.position.xyz;
            position[axis] += body.velocity[axis]*subdt;
            if (!body_collision(position,body.angle.xyz,id,p,cells,atlas)) body.position.xyz = position;
            else {
                if (axis == 1u && body.velocity.y < 0.0f) floor_contact = true;
                body.velocity[axis] *= -0.15f;
            }
        }
    }
    if (floor_contact) {
        body.velocity.xz *= 0.82f;
        body.spin.xyz *= 0.75f;
    }
    if (floor_contact && length(body.velocity.xyz) < 0.35f && length(body.spin.xyz) < 0.25f) ++body.info.z;
    else body.info.z = 0u;
    if (body.info.z > 20u) {
        body.info.w = 1u;
        body.velocity = body.spin = float4(0);
    }
    if(any(body.position.xyz!=old_position) || any(body.angle.xyz!=old_angle)) {
        atomic_store_explicit(stats+17,1u,memory_order_relaxed);
        atomic_store_explicit(stats+21,1u,memory_order_relaxed);
    }
    bodies[id] = body;
}

kernel void clear_overlay(RESOURCES,uint id [[thread_position_in_grid]]) {
    if(geometry_unchanged(stats)) return;
    if (id < volume(p)) atomic_store_explicit(parents+volume(p)+1u+id,0u,memory_order_relaxed);
}

kernel void voxelize_bodies(RESOURCES,uint id [[thread_position_in_grid]]) {
    if(geometry_unchanged(stats)) return;
    uint slot = id/64u, local = id%64u;
    uint count = min(atomic_load_explicit(stats,memory_order_relaxed),p.limits.x);
    if (slot >= count || bodies[slot].info.y == 0u) return;
    uint value = atlas[id];
    if (value == 0u) return;
    Body body = bodies[slot];
    float3 center = body.position.xyz + rotate_xyz(float3(coordinate(local,4u))+0.5f-2.0f,body.angle.xyz);
    int3 c = int3(floor(center));
    if (!inside(c,p)) return;
    // A deterministic material priority avoids first-writer color flicker
    // when rotated fragments overlap the same display cell.
    atomic_fetch_max_explicit(parents+volume(p)+1u+index3(uint3(c),p.world.x),value,memory_order_relaxed);
    // Raster overlap changes visibility only; the body atlas retains all mass.
}

kernel void build_bricks(RESOURCES,uint id [[thread_position_in_grid]]) {
    if(geometry_unchanged(stats)) return;
    uint n = p.config.x;
    if (id >= n*n*n) return;
    uint3 base = coordinate(id,n)*8u;
    uint occupied = 0u;
    for (uint local = 0u; local < 512u; ++local) {
        if (material(int3(base+coordinate(local,8u)),p,cells,parents) != 0u) ++occupied;
    }
    bricks[id] = occupied;
}
