cmake_minimum_required(VERSION 3.20)
project(scene_tonemap_ir LANGUAGES CXX)

set(CMAKE_CXX_STANDARD 20)
set(CMAKE_CXX_STANDARD_REQUIRED ON)

if(NOT DEFINED REPO_ROOT)
    message(FATAL_ERROR "REPO_ROOT is required")
endif()

file(GLOB_RECURSE IR_SOURCES CONFIGURE_DEPENDS
    "${REPO_ROOT}/gpu/vk/eden/shader_recompiler/frontend/ir/*.cpp")

add_executable(scene_tonemap_ir
    "${REPO_ROOT}/tests/scene_tonemap_ir.cpp"
    ${IR_SOURCES})

target_include_directories(scene_tonemap_ir PRIVATE
    "${REPO_ROOT}/gpu/vk/compat"
    "${REPO_ROOT}/gpu/compat"
    "${REPO_ROOT}/gpu/vk/eden"
    "${REPO_ROOT}/gpu/third_party/suyu")

target_compile_definitions(scene_tonemap_ir PRIVATE
    NDEBUG MKWVK_INCLUDES=2 VK_NO_PROTOTYPES)

if(MSVC)
    target_compile_options(scene_tonemap_ir PRIVATE /W0 /O1 /EHsc)
else()
    target_compile_options(scene_tonemap_ir PRIVATE -w -O1)
endif()
