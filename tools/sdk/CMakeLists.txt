cmake_minimum_required(VERSION 3.20)
project(rill_slang_sdk VERSION 0.1.0 LANGUAGES CXX)

include(GNUInstallDirs)
set(SLANG_ROOT "" CACHE PATH "Root of a compatible Slang SDK installation")
# Recent Slang releases separate the compiler from their dispatch library.
if(SLANG_ROOT)
    find_path(SLANG_INCLUDE_DIR slang.h PATHS "${SLANG_ROOT}/include"
        NO_DEFAULT_PATH REQUIRED)
    find_library(SLANG_LIBRARY NAMES slang-compiler slang
        PATHS "${SLANG_ROOT}/lib" NO_DEFAULT_PATH REQUIRED)
else()
    find_path(SLANG_INCLUDE_DIR slang.h REQUIRED)
    find_library(SLANG_LIBRARY NAMES slang-compiler slang REQUIRED)
endif()

add_library(rill_slang SHARED slang.cpp)
target_compile_features(rill_slang PRIVATE cxx_std_17)
target_compile_definitions(rill_slang PRIVATE RILL_SLANG_BUILD SLANG_DYNAMIC)
target_include_directories(rill_slang PRIVATE "${SLANG_INCLUDE_DIR}" include)
target_link_libraries(rill_slang PRIVATE "${SLANG_LIBRARY}")
set_target_properties(rill_slang PROPERTIES
    CXX_VISIBILITY_PRESET hidden
    VISIBILITY_INLINES_HIDDEN YES)
if(APPLE)
    set_target_properties(rill_slang PROPERTIES
        INSTALL_RPATH "@loader_path/../../slang/lib"
        INSTALL_NAME_DIR "@rpath")
elseif(UNIX)
    set_target_properties(rill_slang PROPERTIES
        INSTALL_RPATH "$ORIGIN/../../slang/lib")
else()
    message(FATAL_ERROR "The bundled Slang SDK bridge currently supports macOS and Linux")
endif()
install(TARGETS rill_slang
    LIBRARY DESTINATION "${CMAKE_INSTALL_LIBDIR}"
    RUNTIME DESTINATION "${CMAKE_INSTALL_BINDIR}")
install(FILES include/rill/slang.h DESTINATION "${CMAKE_INSTALL_INCLUDEDIR}/rill")

include(CTest)
if(BUILD_TESTING)
    find_package(Python3 COMPONENTS Interpreter REQUIRED)
    add_test(NAME slang_sdk
        COMMAND "${Python3_EXECUTABLE}" "${CMAKE_CURRENT_SOURCE_DIR}/test_slang.py"
            "$<TARGET_FILE:rill_slang>")
endif()
