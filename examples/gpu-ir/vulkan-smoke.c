// Compile with a Vulkan 1.2+ SDK: cc -std=c11 vulkan-smoke.c -lvulkan -o vulkan-smoke
// Validate with spirv-val --target-env vulkan1.2 clamp.spv. The module must implement
// the rill_map storage-buffer ABI in clamp-vulkan.ll.
// Run: vulkan-smoke clamp.spv     Exit codes: 0 passed, 1 failed, 77 skipped.
#include <vulkan/vulkan.h>
#include <inttypes.h>
#include <stdint.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>

enum { SKIPPED = 77, WORDS = 196, LOCAL_SIZE = 64 };
#define CHECK(call) do { VkResult status = (call); if (status != VK_SUCCESS) { \
    fprintf(stderr, "%s returned %d\n", #call, status); return 1; } } while (0)

typedef struct {
    VkInstance instance;
    VkPhysicalDevice physical;
    VkPhysicalDeviceProperties properties;
    uint32_t family;
    VkDevice device;
    VkQueue queue;
    VkBuffer buffer;
    VkDeviceMemory memory;
    void *mapped;
    VkDeviceSize stride;
    VkDescriptorSetLayout set_layout;
    VkPipelineLayout layout;
    VkShaderModule shader;
    VkPipeline pipeline;
    VkDescriptorPool descriptors;
    VkDescriptorSet set;
    VkCommandPool commands;
    VkCommandBuffer command;
} Host;

static int skip(const char *reason) {
    fprintf(stderr, "SKIP: %s\n", reason);
    return SKIPPED;
}

static int read_shader(const char *path, uint32_t **words, size_t *bytes, VkBool32 *int8) {
    FILE *file = fopen(path, "rb");
    if (!file) { perror(path); return 1; }
    long size = -1;
    if (fseek(file, 0, SEEK_END) == 0) size = ftell(file);
    if (size < 20 || size > 16 * 1024 * 1024 || size % 4 || fseek(file, 0, SEEK_SET)) {
        fclose(file);
        fprintf(stderr, "invalid SPIR-V file size\n");
        return 1;
    }
    *bytes = (size_t)size;
    *words = malloc(*bytes);
    int ok = *words && fread(*words, 1, *bytes, file) == *bytes;
    fclose(file);
    if (!ok || (*words)[0] != 0x07230203) {
        fprintf(stderr, "expected native-endian SPIR-V words\n");
        return 1;
    }
    for (size_t i = 5; i < *bytes / 4;) {
        uint32_t count = (*words)[i] >> 16, opcode = (*words)[i] & 0xffff;
        if (count == 0 || count > *bytes / 4 - i) {
            fprintf(stderr, "malformed SPIR-V instruction\n");
            return 1;
        }
        if (opcode == 17 && count == 2) { // OpCapability
            uint32_t cap = (*words)[i + 1];
            if (cap == 39) *int8 = VK_TRUE; // Int8
            else if (cap != 1) { // Shader
                fprintf(stderr, "unexpected capability %" PRIu32 " for this fixture\n", cap);
                return 1;
            }
        }
        i += count;
    }
    return 0;
}

static int open_device(Host *h, VkBool32 need_int8) {
    VkApplicationInfo app = {.sType=VK_STRUCTURE_TYPE_APPLICATION_INFO, .apiVersion=VK_API_VERSION_1_2};
    VkInstanceCreateInfo instance = {.sType=VK_STRUCTURE_TYPE_INSTANCE_CREATE_INFO, .pApplicationInfo=&app};
    VkResult result = vkCreateInstance(&instance, NULL, &h->instance);
    if (result == VK_ERROR_INCOMPATIBLE_DRIVER) return skip("no compatible Vulkan 1.2 driver");
    CHECK(result);
    uint32_t count = 0;
    CHECK(vkEnumeratePhysicalDevices(h->instance, &count, NULL));
    if (!count) return skip("no Vulkan devices");
    VkPhysicalDevice *devices = calloc(count, sizeof(*devices));
    if (!devices) return 1;
    result = vkEnumeratePhysicalDevices(h->instance, &count, devices);
    if (result != VK_SUCCESS) { free(devices); CHECK(result); }
    for (uint32_t i = 0; i < count && !h->physical; ++i) {
        vkGetPhysicalDeviceProperties(devices[i], &h->properties);
        if (h->properties.apiVersion < VK_API_VERSION_1_2) continue;
        VkPhysicalDeviceVulkan12Features f = {.sType=VK_STRUCTURE_TYPE_PHYSICAL_DEVICE_VULKAN_1_2_FEATURES};
        VkPhysicalDeviceFeatures2 features = {.sType=VK_STRUCTURE_TYPE_PHYSICAL_DEVICE_FEATURES_2, .pNext=&f};
        vkGetPhysicalDeviceFeatures2(devices[i], &features);
        if (need_int8 && !f.shaderInt8) continue;
        uint32_t n = 0;
        vkGetPhysicalDeviceQueueFamilyProperties(devices[i], &n, NULL);
        VkQueueFamilyProperties *families = calloc(n, sizeof(*families));
        if (!families) { free(devices); return 1; }
        vkGetPhysicalDeviceQueueFamilyProperties(devices[i], &n, families);
        for (uint32_t j = 0; j < n; ++j) {
            if (families[j].queueCount && (families[j].queueFlags & VK_QUEUE_COMPUTE_BIT)) {
                h->physical = devices[i];
                h->family = j;
                break;
            }
        }
        free(families);
    }
    free(devices);
    if (!h->physical) return skip(need_int8 ? "no Vulkan 1.2 compute device with shaderInt8" : "no Vulkan 1.2 compute device");
    float priority = 1;
    VkDeviceQueueCreateInfo queue = {.sType=VK_STRUCTURE_TYPE_DEVICE_QUEUE_CREATE_INFO,
        .queueFamilyIndex=h->family, .queueCount=1, .pQueuePriorities=&priority};
    VkPhysicalDeviceVulkan12Features enabled = {.sType=VK_STRUCTURE_TYPE_PHYSICAL_DEVICE_VULKAN_1_2_FEATURES, .shaderInt8=need_int8};
    VkDeviceCreateInfo device = {.sType=VK_STRUCTURE_TYPE_DEVICE_CREATE_INFO, .pNext=&enabled,
        .queueCreateInfoCount=1, .pQueueCreateInfos=&queue};
    CHECK(vkCreateDevice(h->physical, &device, NULL, &h->device));
    vkGetDeviceQueue(h->device, h->family, 0, &h->queue);
    printf("device: %s, shaderInt8 enabled: %u\n", h->properties.deviceName, need_int8);
    return 0;
}

// Three disjoint descriptor ranges share a single host-visible coherent allocation.
static int create_buffer(Host *h) {
    VkDeviceSize alignment = h->properties.limits.minStorageBufferOffsetAlignment;
    VkDeviceSize data_bytes = WORDS * sizeof(int32_t);
    h->stride = (data_bytes + alignment - 1) / alignment * alignment;
    VkBufferCreateInfo info = {.sType=VK_STRUCTURE_TYPE_BUFFER_CREATE_INFO,
        .size=2*h->stride + sizeof(uint32_t), .usage=VK_BUFFER_USAGE_STORAGE_BUFFER_BIT, .sharingMode=VK_SHARING_MODE_EXCLUSIVE};
    CHECK(vkCreateBuffer(h->device, &info, NULL, &h->buffer));
    VkMemoryRequirements requirements;
    vkGetBufferMemoryRequirements(h->device, h->buffer, &requirements);
    VkPhysicalDeviceMemoryProperties properties;
    vkGetPhysicalDeviceMemoryProperties(h->physical, &properties);
    VkMemoryPropertyFlags wanted = VK_MEMORY_PROPERTY_HOST_VISIBLE_BIT | VK_MEMORY_PROPERTY_HOST_COHERENT_BIT;
    uint32_t type = 0;
    for (; type < properties.memoryTypeCount; ++type) {
        if ((requirements.memoryTypeBits & (1u << type)) &&
            (properties.memoryTypes[type].propertyFlags & wanted) == wanted) break;
    }
    if (type == properties.memoryTypeCount) return skip("no host-visible coherent storage-buffer memory");
    VkMemoryAllocateInfo alloc = {.sType=VK_STRUCTURE_TYPE_MEMORY_ALLOCATE_INFO,
        .allocationSize=requirements.size, .memoryTypeIndex=type};
    CHECK(vkAllocateMemory(h->device, &alloc, NULL, &h->memory));
    CHECK(vkBindBufferMemory(h->device, h->buffer, h->memory, 0));
    CHECK(vkMapMemory(h->device, h->memory, 0, info.size, 0, &h->mapped));
    return 0;
}

static int create_pipeline(Host *h, const uint32_t *code, size_t size) {
    VkDescriptorSetLayoutBinding bindings[3];
    for (uint32_t i=0; i<3; ++i)
        bindings[i] = (VkDescriptorSetLayoutBinding){.binding=i, .descriptorType=VK_DESCRIPTOR_TYPE_STORAGE_BUFFER,
            .descriptorCount=1, .stageFlags=VK_SHADER_STAGE_COMPUTE_BIT};
    VkDescriptorSetLayoutCreateInfo layout = {.sType=VK_STRUCTURE_TYPE_DESCRIPTOR_SET_LAYOUT_CREATE_INFO,
        .bindingCount=3, .pBindings=bindings};
    CHECK(vkCreateDescriptorSetLayout(h->device, &layout, NULL, &h->set_layout));
    VkPipelineLayoutCreateInfo pipe_layout = {.sType=VK_STRUCTURE_TYPE_PIPELINE_LAYOUT_CREATE_INFO,
        .setLayoutCount=1, .pSetLayouts=&h->set_layout};
    CHECK(vkCreatePipelineLayout(h->device, &pipe_layout, NULL, &h->layout));
    VkShaderModuleCreateInfo shader = {.sType=VK_STRUCTURE_TYPE_SHADER_MODULE_CREATE_INFO, .codeSize=size, .pCode=code};
    CHECK(vkCreateShaderModule(h->device, &shader, NULL, &h->shader));
    VkComputePipelineCreateInfo pipe = {.sType=VK_STRUCTURE_TYPE_COMPUTE_PIPELINE_CREATE_INFO, .layout=h->layout,
        .stage={.sType=VK_STRUCTURE_TYPE_PIPELINE_SHADER_STAGE_CREATE_INFO, .stage=VK_SHADER_STAGE_COMPUTE_BIT,
            .module=h->shader, .pName="rill_map"}};
    CHECK(vkCreateComputePipelines(h->device, VK_NULL_HANDLE, 1, &pipe, NULL, &h->pipeline));
    VkDescriptorPoolSize capacity = {VK_DESCRIPTOR_TYPE_STORAGE_BUFFER, 3};
    VkDescriptorPoolCreateInfo descriptors = {.sType=VK_STRUCTURE_TYPE_DESCRIPTOR_POOL_CREATE_INFO,
        .maxSets=1, .poolSizeCount=1, .pPoolSizes=&capacity};
    CHECK(vkCreateDescriptorPool(h->device, &descriptors, NULL, &h->descriptors));
    VkDescriptorSetAllocateInfo set = {.sType=VK_STRUCTURE_TYPE_DESCRIPTOR_SET_ALLOCATE_INFO,
        .descriptorPool=h->descriptors, .descriptorSetCount=1, .pSetLayouts=&h->set_layout};
    CHECK(vkAllocateDescriptorSets(h->device, &set, &h->set));
    for (uint32_t i=0; i<3; ++i) {
        VkDescriptorBufferInfo buffer = {h->buffer, i*h->stride, i==2 ? sizeof(uint32_t) : WORDS*sizeof(int32_t)};
        VkWriteDescriptorSet write = {.sType=VK_STRUCTURE_TYPE_WRITE_DESCRIPTOR_SET, .dstSet=h->set, .dstBinding=i,
            .descriptorCount=1, .descriptorType=VK_DESCRIPTOR_TYPE_STORAGE_BUFFER, .pBufferInfo=&buffer};
        vkUpdateDescriptorSets(h->device, 1, &write, 0, NULL);
    }
    VkCommandPoolCreateInfo pool = {.sType=VK_STRUCTURE_TYPE_COMMAND_POOL_CREATE_INFO, .queueFamilyIndex=h->family};
    CHECK(vkCreateCommandPool(h->device, &pool, NULL, &h->commands));
    VkCommandBufferAllocateInfo command = {.sType=VK_STRUCTURE_TYPE_COMMAND_BUFFER_ALLOCATE_INFO,
        .commandPool=h->commands, .level=VK_COMMAND_BUFFER_LEVEL_PRIMARY, .commandBufferCount=1};
    CHECK(vkAllocateCommandBuffers(h->device, &command, &h->command));
    return 0;
}

static int32_t input_value(uint32_t i) {
    switch (i % 7) {
    case 0: return INT32_MIN;
    case 1: return -(int32_t)i;
    case 2: return 0;
    case 3: return (int32_t)i + 1;
    case 4: return INT32_MAX;
    case 5: return -1;
    default: return (int32_t)(17*i + 42);
    }
}

static int run_count(Host *h, uint32_t count) {
    unsigned char *base = h->mapped;
    int32_t *input = (int32_t *)base, *output = (int32_t *)(base + h->stride);
    uint32_t *length = (uint32_t *)(base + 2*h->stride);
    const int32_t canary = -7777;
    *length = count;
    for (uint32_t i=0; i<WORDS; ++i) { input[i]=input_value(i); output[i]=canary; }
    CHECK(vkResetCommandPool(h->device, h->commands, 0));
    VkCommandBufferBeginInfo begin = {.sType=VK_STRUCTURE_TYPE_COMMAND_BUFFER_BEGIN_INFO};
    CHECK(vkBeginCommandBuffer(h->command, &begin));
    vkCmdBindPipeline(h->command, VK_PIPELINE_BIND_POINT_COMPUTE, h->pipeline);
    vkCmdBindDescriptorSets(h->command, VK_PIPELINE_BIND_POINT_COMPUTE, h->layout, 0, 1, &h->set, 0, NULL);
    uint32_t groups = count ? (count+LOCAL_SIZE-1)/LOCAL_SIZE : 1;
    vkCmdDispatch(h->command, groups, 1, 1);
    VkMemoryBarrier barrier = {.sType=VK_STRUCTURE_TYPE_MEMORY_BARRIER,
        .srcAccessMask=VK_ACCESS_SHADER_WRITE_BIT, .dstAccessMask=VK_ACCESS_HOST_READ_BIT};
    vkCmdPipelineBarrier(h->command, VK_PIPELINE_STAGE_COMPUTE_SHADER_BIT, VK_PIPELINE_STAGE_HOST_BIT,
                         0, 1, &barrier, 0, NULL, 0, NULL);
    CHECK(vkEndCommandBuffer(h->command));
    VkSubmitInfo submit = {.sType=VK_STRUCTURE_TYPE_SUBMIT_INFO, .commandBufferCount=1, .pCommandBuffers=&h->command};
    CHECK(vkQueueSubmit(h->queue, 1, &submit, VK_NULL_HANDLE));
    CHECK(vkQueueWaitIdle(h->queue));
    for (uint32_t i=0; i<WORDS; ++i) {
        int32_t v = input_value(i), expected = i<count ? (v<0 ? 0 : v) : canary;
        if (input[i]!=v || output[i]!=expected || *length!=count) {
            fprintf(stderr, "count=%" PRIu32 ", lane=%" PRIu32 ", expected=%" PRId32 ", output=%" PRId32 "\n",
                    count, i, expected, output[i]);
            return 1;
        }
    }
    printf("PASS count=%" PRIu32 ", workgroups=%" PRIu32 "\n", count, groups);
    return 0;
}

static void close_host(Host *h) {
    if (h->device) {
        vkDestroyCommandPool(h->device, h->commands, NULL);
        vkDestroyDescriptorPool(h->device, h->descriptors, NULL);
        vkDestroyPipeline(h->device, h->pipeline, NULL);
        vkDestroyShaderModule(h->device, h->shader, NULL);
        vkDestroyPipelineLayout(h->device, h->layout, NULL);
        vkDestroyDescriptorSetLayout(h->device, h->set_layout, NULL);
        if (h->mapped) vkUnmapMemory(h->device, h->memory);
        vkDestroyBuffer(h->device, h->buffer, NULL);
        vkFreeMemory(h->device, h->memory, NULL);
        vkDestroyDevice(h->device, NULL);
    }
    if (h->instance) vkDestroyInstance(h->instance, NULL);
}

int main(int argc, char **argv) {
    if (argc != 2) { fprintf(stderr, "usage: vulkan-smoke clamp.spv\n"); return 1; }
    Host host = {0};
    uint32_t *code = NULL;
    size_t size = 0;
    VkBool32 need_int8 = VK_FALSE;
    int status = read_shader(argv[1], &code, &size, &need_int8);
    if (!status) status = open_device(&host, need_int8);
    if (!status) status = create_buffer(&host);
    if (!status) status = create_pipeline(&host, code, size);
    uint32_t counts[] = {0, 1, 5, 63, 64, 65, 129};
    for (size_t i=0; !status && i<sizeof(counts)/sizeof(counts[0]); ++i)
        status = run_count(&host, counts[i]);
    close_host(&host);
    free(code);
    return status;
}
