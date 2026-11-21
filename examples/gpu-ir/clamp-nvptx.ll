; Handwritten CUDA lowering for signed i32 clamp. The host supplies disjoint
; device buffers with at least %length aligned i32 elements, one-dimensional
; grid dimensions, and exactly 64 threads per block.
target triple = "nvptx64-nvidia-cuda"

declare i32 @llvm.nvvm.read.ptx.sreg.ctaid.x()
declare i32 @llvm.nvvm.read.ptx.sreg.tid.x()

define ptx_kernel void @rill_map(
    ptr addrspace(1) readonly align 4 %input,
    ptr addrspace(1) writeonly align 4 %output,
    i32 %length) "nvvm.reqntid"="64,1,1" {
entry:
  %block = call i32 @llvm.nvvm.read.ptx.sreg.ctaid.x()
  %thread = call i32 @llvm.nvvm.read.ptx.sreg.tid.x()
  %block64 = zext i32 %block to i64
  %thread64 = zext i32 %thread to i64
  %base = mul i64 %block64, 64
  %index = add i64 %base, %thread64
  %length64 = zext i32 %length to i64
  %valid = icmp ult i64 %index, %length64
  br i1 %valid, label %map, label %done

map:
  %src = getelementptr i32, ptr addrspace(1) %input, i64 %index
  %input_value = load i32, ptr addrspace(1) %src, align 4
; begin rill.map
  %negative = icmp slt i32 %input_value, 0
  %mapped_value = select i1 %negative, i32 0, i32 %input_value
; end rill.map
  %dst = getelementptr i32, ptr addrspace(1) %output, i64 %index
  store i32 %mapped_value, ptr addrspace(1) %dst, align 4
  br label %done

done:
  ret void
}
