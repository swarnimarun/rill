; LLVM 22.1.8 SPIR-V backend, Vulkan 1.2 compute profile.
; Set 0 storage buffers: binding 0 = input i32[], 1 = output i32[],
; 2 = one u32 count. Both data descriptors must cover at least count words.
; Dispatch ceil(count / 64) workgroups in X, with Y = Z = 1.
; LLVM 22.1.8 emits the resource names as i8 constants; its module declares
; Int8, so query and enable shaderInt8 before creating the Vulkan pipeline.
; Build with clang -cc1 -triple spirv1.5-unknown-vulkan1.2-compute -x ir
; -emit-obj clamp-vulkan.ll -o clamp.spv; validate with
; spirv-val --target-env vulkan1.2 clamp.spv.
target triple = "spirv1.5-unknown-vulkan1.2-compute"

@input_name = private unnamed_addr constant [6 x i8] c"input\00"
@output_name = private unnamed_addr constant [7 x i8] c"output\00"
@count_name = private unnamed_addr constant [6 x i8] c"count\00"

define void @rill_map() #0 {
entry:
  %i = call i32 @llvm.spv.thread.id.i32(i32 0)
  %counts = call target("spirv.VulkanBuffer", [0 x i32], 12, 0) @llvm.spv.resource.handlefrombinding.tspirv.VulkanBuffer_a0i32_12_0t(i32 0, i32 2, i32 1, i32 0, ptr @count_name)
  %count_ptr = call ptr addrspace(11) @llvm.spv.resource.getpointer.p11.tspirv.VulkanBuffer_a0i32_12_0t(target("spirv.VulkanBuffer", [0 x i32], 12, 0) %counts, i32 0)
  %count = load i32, ptr addrspace(11) %count_ptr, align 4
  %active = icmp ult i32 %i, %count
  br i1 %active, label %map, label %done

map:
  %input = call target("spirv.VulkanBuffer", [0 x i32], 12, 0) @llvm.spv.resource.handlefrombinding.tspirv.VulkanBuffer_a0i32_12_0t(i32 0, i32 0, i32 1, i32 0, ptr @input_name)
  %input_ptr = call ptr addrspace(11) @llvm.spv.resource.getpointer.p11.tspirv.VulkanBuffer_a0i32_12_0t(target("spirv.VulkanBuffer", [0 x i32], 12, 0) %input, i32 %i)
  %input_value = load i32, ptr addrspace(11) %input_ptr, align 4
; begin rill.map
  %negative = icmp slt i32 %input_value, 0
  %mapped_value = select i1 %negative, i32 0, i32 %input_value
; end rill.map
  %output = call target("spirv.VulkanBuffer", [0 x i32], 12, 1) @llvm.spv.resource.handlefrombinding.tspirv.VulkanBuffer_a0i32_12_1t(i32 0, i32 1, i32 1, i32 0, ptr @output_name)
  %output_ptr = call ptr addrspace(11) @llvm.spv.resource.getpointer.p11.tspirv.VulkanBuffer_a0i32_12_1t(target("spirv.VulkanBuffer", [0 x i32], 12, 1) %output, i32 %i)
  store i32 %mapped_value, ptr addrspace(11) %output_ptr, align 4
  br label %done

done:
  ret void
}

declare i32 @llvm.spv.thread.id.i32(i32)
declare target("spirv.VulkanBuffer", [0 x i32], 12, 0) @llvm.spv.resource.handlefrombinding.tspirv.VulkanBuffer_a0i32_12_0t(i32, i32, i32, i32, ptr)
declare target("spirv.VulkanBuffer", [0 x i32], 12, 1) @llvm.spv.resource.handlefrombinding.tspirv.VulkanBuffer_a0i32_12_1t(i32, i32, i32, i32, ptr)
declare ptr addrspace(11) @llvm.spv.resource.getpointer.p11.tspirv.VulkanBuffer_a0i32_12_0t(target("spirv.VulkanBuffer", [0 x i32], 12, 0), i32)
declare ptr addrspace(11) @llvm.spv.resource.getpointer.p11.tspirv.VulkanBuffer_a0i32_12_1t(target("spirv.VulkanBuffer", [0 x i32], 12, 1), i32)
attributes #0 = { "hlsl.shader"="compute" "hlsl.numthreads"="64,1,1" }
