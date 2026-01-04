cmake_minimum_required(VERSION 3.15)
project(nesium-sdl2-setup)

# Download and include CPM
set(CPM_DOWNLOAD_VERSION 0.38.7)

if(CPM_SOURCE_CACHE)
  set(CPM_DOWNLOAD_LOCATION "${CPM_SOURCE_CACHE}/cpm/CPM_${CPM_DOWNLOAD_VERSION}.cmake")
elseif(DEFINED ENV{CPM_SOURCE_CACHE})
  set(CPM_DOWNLOAD_LOCATION "$ENV{CPM_SOURCE_CACHE}/cpm/CPM_${CPM_DOWNLOAD_VERSION}.cmake")
else()
  set(CPM_DOWNLOAD_LOCATION "${CMAKE_BINARY_DIR}/cmake/CPM_${CPM_DOWNLOAD_VERSION}.cmake")
endif()

# Expand relative path. This is important if the provided path contains a tilde (~)
get_filename_component(CPM_DOWNLOAD_LOCATION ${CPM_DOWNLOAD_LOCATION} ABSOLUTE)

function(download_cpm)
  message(STATUS "Downloading CPM.cmake to ${CPM_DOWNLOAD_LOCATION}")
  file(DOWNLOAD
       https://github.com/cpm-cmake/CPM.cmake/releases/download/v${CPM_DOWNLOAD_VERSION}/CPM.cmake
       ${CPM_DOWNLOAD_LOCATION}
  )
endfunction()

if(NOT (EXISTS ${CPM_DOWNLOAD_LOCATION}))
  download_cpm()
else()
  # resume download if it previously failed
  file(READ ${CPM_DOWNLOAD_LOCATION} check)
  if("${check}" STREQUAL "")
    download_cpm()
  endif()
endif()

include(${CPM_DOWNLOAD_LOCATION})

# Use CPM to download and build SDL2
CPMAddPackage(
    NAME SDL2
    GITHUB_REPOSITORY libsdl-org/SDL
    GIT_TAG release-2.30.5
    OPTIONS
        "SDL_SHARED OFF"
        "SDL_STATIC ON"
        "SDL_TEST OFF"
)

if(SDL2_FOUND)
    message(STATUS "SDL2 found at: ${SDL2_DIR}")
    message(STATUS "SDL2 libraries: ${SDL2_LIBRARIES}")
    message(STATUS "SDL2 include dir: ${SDL2_INCLUDE_DIR}")
    
    # Print instructions for Rust
    message(STATUS "")
    message(STATUS "To use this SDL2 build with Rust, set:")
    message(STATUS "  set SDL2_DIR=${CMAKE_BINARY_DIR}/_deps/sdl2-src")
    message(STATUS "  set LIB=%LIB%;${CMAKE_BINARY_DIR}/_deps/sdl2-build")
endif()

