cmake_minimum_required(VERSION 3.20)
project(agent_coreutils VERSION 0.1.0 LANGUAGES CXX)

set(CMAKE_CXX_STANDARD 20)
set(CMAKE_CXX_STANDARD_REQUIRED ON)
set(CMAKE_CXX_EXTENSIONS OFF)
if(NOT CMAKE_BUILD_TYPE AND NOT CMAKE_CONFIGURATION_TYPES)
  set(CMAKE_BUILD_TYPE Release)
endif()

# The agent guide is embedded in the binary (`acu agent-guide`) from docs/AGENT_GUIDE.md.
set_property(DIRECTORY APPEND PROPERTY CMAKE_CONFIGURE_DEPENDS ${CMAKE_SOURCE_DIR}/docs/AGENT_GUIDE.md)
file(READ ${CMAKE_SOURCE_DIR}/docs/AGENT_GUIDE.md ACU_GUIDE_TEXT)
file(WRITE ${CMAKE_BINARY_DIR}/gen/agent_guide.inc "R\"ACUGUIDE(${ACU_GUIDE_TEXT})ACUGUIDE\"")

file(GLOB_RECURSE ACU_LIB_SOURCES CONFIGURE_DEPENDS
  ${CMAKE_SOURCE_DIR}/src/core/*.cpp
  ${CMAKE_SOURCE_DIR}/src/cmd/*.cpp
  ${CMAKE_SOURCE_DIR}/src/index/*.cpp
  ${CMAKE_SOURCE_DIR}/src/mcp/*.cpp)

add_library(acu_core STATIC ${ACU_LIB_SOURCES})
target_include_directories(acu_core PUBLIC ${CMAKE_SOURCE_DIR}/src ${CMAKE_BINARY_DIR}/gen)
target_compile_definitions(acu_core PUBLIC ACU_VERSION="${PROJECT_VERSION}")

find_package(Threads REQUIRED)
target_link_libraries(acu_core PUBLIC Threads::Threads)

if(MSVC)
  target_compile_options(acu_core PUBLIC /utf-8 /W3 /O2 /EHsc /MT$<$<CONFIG:Debug>:d>)
  target_compile_definitions(acu_core PUBLIC _CRT_SECURE_NO_WARNINGS NOMINMAX WIN32_LEAN_AND_MEAN)
else()
  target_compile_options(acu_core PUBLIC -Wall -Wextra -Wno-unused-parameter)
  if(NOT APPLE)
    target_link_options(acu_core PUBLIC -static -static-libgcc -static-libstdc++)
  endif()
endif()
if(WIN32)
  target_compile_definitions(acu_core PUBLIC NOMINMAX WIN32_LEAN_AND_MEAN UNICODE _UNICODE)
  target_link_libraries(acu_core PUBLIC shell32 advapi32)
endif()

add_executable(acu src/main.cpp)
target_link_libraries(acu PRIVATE acu_core)

enable_testing()
add_executable(acu_tests tests/tests.cpp)
target_link_libraries(acu_tests PRIVATE acu_core)
add_test(NAME acu_tests COMMAND acu_tests)
