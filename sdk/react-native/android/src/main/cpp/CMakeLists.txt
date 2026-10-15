cmake_minimum_required(VERSION 3.22.1)
project(swarmie_mobile)
add_library(swarmie_client_ffi SHARED IMPORTED)
set_target_properties(swarmie_client_ffi PROPERTIES IMPORTED_LOCATION
  "${CMAKE_CURRENT_SOURCE_DIR}/../jniLibs/${ANDROID_ABI}/libswarmie_client_ffi.so")
add_library(swarmie_mobile SHARED bridge.cpp)
target_link_libraries(swarmie_mobile swarmie_client_ffi)
