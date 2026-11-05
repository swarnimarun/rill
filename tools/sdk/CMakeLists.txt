cmake_minimum_required(VERSION 3.20)
project(rill_sdk VERSION 0.1.0 LANGUAGES C CXX)

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

set(RAYLIB_ROOT "" CACHE PATH "Root of an optional Raylib installation")
if(RAYLIB_ROOT)
    find_path(RAYLIB_INCLUDE_DIR raylib.h PATHS "${RAYLIB_ROOT}/include"
        NO_DEFAULT_PATH REQUIRED)
    find_library(RAYLIB_LIBRARY NAMES raylib PATHS "${RAYLIB_ROOT}/lib"
        NO_DEFAULT_PATH REQUIRED)
    add_library(rill_raylib SHARED raylib.c)
    target_compile_features(rill_raylib PRIVATE c_std_11)
    target_compile_definitions(rill_raylib PRIVATE RILL_RAYLIB_BUILD USE_LIBTYPE_SHARED)
    target_include_directories(rill_raylib PRIVATE "${RAYLIB_INCLUDE_DIR}" include)
    target_link_libraries(rill_raylib PRIVATE "${RAYLIB_LIBRARY}")
    set_target_properties(rill_raylib PROPERTIES C_EXTENSIONS NO
        C_VISIBILITY_PRESET hidden)
    if(APPLE)
        set_target_properties(rill_raylib PROPERTIES
            INSTALL_RPATH "@loader_path/../../raylib/lib" INSTALL_NAME_DIR "@rpath")
    elseif(UNIX)
        set_target_properties(rill_raylib PROPERTIES
            INSTALL_RPATH "$ORIGIN/../../raylib/lib")
        target_link_libraries(rill_raylib PRIVATE m)
    endif()
    install(TARGETS rill_raylib
        LIBRARY DESTINATION "${CMAKE_INSTALL_LIBDIR}"
        RUNTIME DESTINATION "${CMAKE_INSTALL_BINDIR}")
    install(FILES include/rill/raylib.h DESTINATION "${CMAKE_INSTALL_INCLUDEDIR}/rill")
endif()

include(CTest)
if(BUILD_TESTING)
    find_package(Python3 COMPONENTS Interpreter REQUIRED)
    add_test(NAME slang_sdk
        COMMAND "${Python3_EXECUTABLE}" "${CMAKE_CURRENT_SOURCE_DIR}/test_slang.py"
            "$<TARGET_FILE:rill_slang>")
    if(TARGET rill_raylib)
        add_executable(raylib_sdk_tests test_raylib.c)
        target_compile_features(raylib_sdk_tests PRIVATE c_std_11)
        target_include_directories(raylib_sdk_tests PRIVATE include "${RAYLIB_INCLUDE_DIR}")
        target_link_libraries(raylib_sdk_tests PRIVATE rill_raylib "${RAYLIB_LIBRARY}")
        set_target_properties(raylib_sdk_tests PROPERTIES C_EXTENSIONS NO)
        add_test(NAME raylib_sdk COMMAND raylib_sdk_tests)
    endif()
endif()
