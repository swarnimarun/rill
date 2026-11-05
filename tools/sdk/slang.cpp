#include "rill/slang.h"
#include <slang.h>
#include <slang-com-ptr.h>

#include <cerrno>
#include <cstdio>
#include <cstring>
#include <exception>
#include <filesystem>
#include <string>
#include <vector>
#include <unistd.h>

namespace {
using Slang::ComPtr;
namespace fs = std::filesystem;

int32_t error(int32_t status, const char* message) {
    std::fprintf(stderr, "Rill Slang SDK: %s\n", message);
    return status;
}

void diagnostics(const ComPtr<slang::IBlob>& blob) {
    if (blob && blob->getBufferSize()) {
        std::fwrite(blob->getBufferPointer(), 1, blob->getBufferSize(), stderr);
    }
}

bool identifier(const char* text) {
    if (!text || !*text) return false;
    auto letter = [](unsigned char c) {
        return c == '_' || (c >= 'a' && c <= 'z') || (c >= 'A' && c <= 'Z');
    };
    if (!letter(static_cast<unsigned char>(*text))) return false;
    for (const unsigned char* p = reinterpret_cast<const unsigned char*>(text); *p; ++p) {
        if (!letter(*p) && (*p < '0' || *p > '9')) return false;
    }
    return true;
}

// The temporary file is private, created exclusively in the destination
// directory and renamed only after a complete write. Existing output survives
// compilation and I/O failures. No caller-controlled text reaches a shell.
class OutputFile {
public:
    explicit OutputFile(const fs::path& output) {
        std::string name = output.string() + ".rill-slang-XXXXXX";
        name_.assign(name.begin(), name.end());
        name_.push_back('\0');
        descriptor_ = mkstemp(name_.data());
        if (descriptor_ < 0) name_.clear();
    }
    ~OutputFile() {
        if (descriptor_ >= 0) close(descriptor_);
        if (!name_.empty()) unlink(name_.data());
    }
    bool publish(const fs::path& output, const void* data, size_t size) {
        if (descriptor_ < 0) return false;
        const char* cursor = static_cast<const char*>(data);
        while (size) {
            ssize_t written = write(descriptor_, cursor, size);
            if (written < 0 && errno == EINTR) continue;
            if (written <= 0) return false;
            cursor += written;
            size -= static_cast<size_t>(written);
        }
        if (fsync(descriptor_) != 0) return false;
        int fd = descriptor_;
        descriptor_ = -1;
        if (close(fd) != 0) return false;
        if (rename(name_.data(), output.c_str()) != 0) return false;
        name_.clear();
        return true;
    }
    OutputFile(const OutputFile&) = delete;
    OutputFile& operator=(const OutputFile&) = delete;

private:
    std::vector<char> name_;
    int descriptor_ = -1;
};

int32_t compile(const char* source, const char* entry_name, int32_t stage_code,
    const char* output, bool metal) {
    if (!source || !*source || !identifier(entry_name) || !output || !*output
        || stage_code < 0 || stage_code > 2) {
        return error(1, "expected source/output paths, entry identifier and stage 0, 1 or 2");
    }
    static const SlangStage stages[] = { SLANG_STAGE_VERTEX, SLANG_STAGE_FRAGMENT, SLANG_STAGE_COMPUTE };
    SlangStage stage = stages[stage_code];
    std::error_code ec;
    fs::path source_path = fs::canonical(fs::path(source), ec);
    if (ec || !fs::is_regular_file(source_path, ec)) return error(2, "source is not a readable regular file");
    fs::path output_path = fs::absolute(fs::path(output));
    if (fs::equivalent(source_path, output_path, ec)) return error(1, "output must differ from source");
    std::string path = source_path.string();
    std::string include = source_path.parent_path().string();
    const char* search_paths[] = { include.c_str() };

    ComPtr<slang::IGlobalSession> global;
    if (SLANG_FAILED(slang::createGlobalSession(global.writeRef())) || !global) {
        return error(2, "cannot create Slang global session");
    }
    slang::CompilerOptionEntry named_entry = {};
    named_entry.name = slang::CompilerOptionName::VulkanUseEntryPointName;
    named_entry.value.kind = slang::CompilerOptionValueKind::Int;
    named_entry.value.intValue0 = 1;
    slang::TargetDesc target = {};
    target.format = metal ? SLANG_METAL : SLANG_SPIRV;
    target.profile = global->findProfile(metal ? "sm_6_0" : "spirv_1_0");
    target.compilerOptionEntries = &named_entry;
    target.compilerOptionEntryCount = 1;
    if (target.profile == SLANG_PROFILE_UNKNOWN) return error(2, "required Slang profile is unavailable");
    slang::SessionDesc description = {};
    description.targets = &target;
    description.targetCount = 1;
    description.defaultMatrixLayoutMode = SLANG_MATRIX_LAYOUT_COLUMN_MAJOR;
    description.searchPaths = search_paths;
    description.searchPathCount = 1;
    ComPtr<slang::ISession> session;
    if (SLANG_FAILED(global->createSession(description, session.writeRef())) || !session) {
        return error(2, "cannot create Slang compilation session");
    }
    ComPtr<slang::IBlob> messages;
    slang::IModule* module = session->loadModuleFromSource("rill_sdk_input", path.c_str(), nullptr, messages.writeRef());
    diagnostics(messages);
    if (!module) return error(2, "module compilation failed");
    ComPtr<slang::IEntryPoint> entry;
    SlangResult result = module->findAndCheckEntryPoint(entry_name, stage, entry.writeRef(), messages.writeRef());
    diagnostics(messages);
    if (SLANG_FAILED(result) || !entry) return error(2, "entry point or stage check failed");
    slang::IComponentType* components[] = { module, entry.get() };
    ComPtr<slang::IComponentType> composite;
    result = session->createCompositeComponentType(components, 2, composite.writeRef(), messages.writeRef());
    diagnostics(messages);
    if (SLANG_FAILED(result) || !composite) return error(2, "program composition failed");
    ComPtr<slang::IComponentType> linked;
    result = composite->link(linked.writeRef(), messages.writeRef());
    diagnostics(messages);
    if (SLANG_FAILED(result) || !linked) return error(2, "program linking failed");
    slang::ProgramLayout* layout = linked->getLayout(0, messages.writeRef());
    diagnostics(messages);
    if (!layout || layout->getEntryPointCount() != 1 || layout->getEntryPointByIndex(0)->getStage() != stage) {
        return error(2, "compiled entry point has the wrong stage");
    }
    ComPtr<slang::IBlob> code;
    result = linked->getEntryPointCode(0, 0, code.writeRef(), messages.writeRef());
    diagnostics(messages);
    if (SLANG_FAILED(result) || !code || !code->getBufferSize()) return error(2, "target generation failed");
    OutputFile file(output_path);
    if (!file.publish(output_path, code->getBufferPointer(), code->getBufferSize())) {
        std::fprintf(stderr, "Rill Slang SDK output: %s\n", std::strerror(errno));
        return 3;
    }
    return 0;
}

int32_t guarded_compile(const char* source, const char* entry, int32_t stage,
    const char* output, bool metal) {
    try {
        return compile(source, entry, stage, output, metal);
    } catch (const fs::filesystem_error& exception) {
        std::fprintf(stderr, "Rill Slang SDK filesystem: %s\n", exception.what());
        return 3;
    } catch (const std::exception& exception) {
        std::fprintf(stderr, "Rill Slang SDK exception: %s\n", exception.what());
        return 2;
    } catch (...) {
        return error(2, "unknown compiler exception");
    }
}
} // namespace

extern "C" int32_t RillSlang_compile_spirv(const char* source, const char* entry,
    int32_t stage, const char* output) {
    return guarded_compile(source, entry, stage, output, false);
}

extern "C" int32_t RillSlang_compile_msl(const char* source, const char* entry,
    int32_t stage, const char* output) {
    return guarded_compile(source, entry, stage, output, true);
}
