"""Execute this directory's clamp PTX on CUDA device 0 and check every word."""

import argparse
import ctypes as c
from contextlib import ExitStack
import os
from pathlib import Path
import sys


def run(image):
    if c.sizeof(c.c_void_p) != 8:
        raise RuntimeError("the nvptx64 smoke test requires a 64-bit host process")
    driver = c.WinDLL("nvcuda.dll") if os.name == "nt" else c.CDLL("libcuda.so.1")
    device_ptr = c.c_uint64
    handle = c.c_void_p
    uint = c.c_uint
    error_string = driver.cuGetErrorString
    error_string.argtypes = [c.c_int, c.POINTER(c.c_char_p)]
    error_string.restype = c.c_int

    def api(name, *types):
        function = getattr(driver, name)
        function.argtypes = types
        function.restype = c.c_int

        def call(*args):
            status = function(*args)
            if status:
                description = c.c_char_p()
                error_string(status, c.byref(description))
                message = (description.value or b"unknown error").decode(errors="replace")
                raise RuntimeError(f"{name} returned {status}: {message}")

        return call

    api("cuInit", uint)(0)
    device = c.c_int()
    api("cuDeviceGet", c.POINTER(c.c_int), c.c_int)(c.byref(device), 0)
    name = c.create_string_buffer(256)
    api("cuDeviceGetName", handle, c.c_int, c.c_int)(name, len(name), device)
    version = c.c_int()
    api("cuDriverGetVersion", c.POINTER(c.c_int))(c.byref(version))

    with ExitStack() as cleanup:
        context = handle()
        api("cuDevicePrimaryCtxRetain", c.POINTER(handle), c.c_int)(c.byref(context), device)
        cleanup.callback(api("cuDevicePrimaryCtxRelease_v2", c.c_int), device)
        api("cuCtxSetCurrent", handle)(context)
        module = handle()
        ptx = c.create_string_buffer(image.read_bytes())
        api("cuModuleLoadDataEx", c.POINTER(handle), handle, uint, handle, handle)(
            c.byref(module), ptx, 0, None, None
        )
        cleanup.callback(api("cuModuleUnload", handle), module)
        kernel = handle()
        api("cuModuleGetFunction", c.POINTER(handle), handle, c.c_char_p)(
            c.byref(kernel), module, b"rill_map"
        )

        lanes = 192
        words = c.c_int32 * (lanes + 2)
        width = c.sizeof(words)
        bases = []
        for _ in range(2):
            base = device_ptr()
            api("cuMemAlloc_v2", c.POINTER(device_ptr), c.c_size_t)(c.byref(base), width)
            cleanup.callback(api("cuMemFree_v2", device_ptr), base)
            bases.append(base)

        # Each argument is a host address from which CUDA copies its value.
        # Input and output occupy independent allocations with a guard at each end.
        source, output = [device_ptr(base.value + c.sizeof(c.c_int32)) for base in bases]
        length = c.c_uint32()
        params = (handle * 3)(*[c.cast(c.byref(v), handle) for v in (source, output, length)])
        to_device = api("cuMemcpyHtoD_v2", device_ptr, handle, c.c_size_t)
        from_device = api("cuMemcpyDtoH_v2", handle, device_ptr, c.c_size_t)
        launch = api("cuLaunchKernel", handle, uint, uint, uint, uint, uint, uint, uint,
                     handle, c.POINTER(handle), handle)
        synchronize = api("cuCtxSynchronize")
        cases = (-2147483648, -1, 0, 1, 42, 2147483647, -2147483647)
        inputs = [111] + [cases[i % len(cases)] for i in range(lanes)] + [222]
        counts = (0, 1, 63, 64, 65, 127, 129, 131, 192)
        for n in counts:
            length.value = n
            initial = [-888] * (lanes + 2)
            to_device(bases[0], words(*inputs), width)
            to_device(bases[1], words(*initial), width)
            launch(kernel, 3, 1, 1, 64, 1, 1, 0, None, params, None)
            synchronize()
            expected = initial.copy()
            expected[1:n + 1] = [max(0, x) for x in inputs[1:n + 1]]
            for label, base, want in zip(("input", "output"), bases, (inputs, expected)):
                observed = words()
                from_device(observed, base, width)
                for offset, (actual, correct) in enumerate(zip(observed, want)):
                    if actual != correct:
                        raise RuntimeError(
                            f"length {n}, {label}[{offset - 1}]: expected {correct}, got {actual}"
                        )
    print(f"{image.name} | {name.value.decode()} | CUDA driver API {version.value} | "
          f"{len(counts)} clamp launches passed, including canaries and signed extremes")


if __name__ == "__main__":
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("ptx", nargs="+", type=Path, help="PTX module with the rill_map clamp kernel")
    try:
        for path in parser.parse_args().ptx:
            run(path)
    except (OSError, RuntimeError) as error:
        sys.exit(str(error))
