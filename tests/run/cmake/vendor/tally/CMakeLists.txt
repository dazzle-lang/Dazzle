# A library only CMake knows how to build, for tests/run/cmake (spec §15.1, D-0090). Its
# configure step writes `tally_config.h` from the options the recipe passes, and its header
# includes that file, so it can be read only from what the build installed.
cmake_minimum_required(VERSION 3.16)
project(tally VERSION 1.0 LANGUAGES C)
include(GNUInstallDirs)

set(TALLY_STEP 1 CACHE STRING "what tally_add adds each time")
option(TALLY_LOUD "whether tally_loud says so" OFF)

configure_file(tally_config.h.in ${CMAKE_CURRENT_BINARY_DIR}/tally_config.h)
add_library(tally tally.c)
target_include_directories(tally PRIVATE ${CMAKE_CURRENT_SOURCE_DIR} ${CMAKE_CURRENT_BINARY_DIR})
# What the pkg-config file's Cflags hand every user of the header, which checks for it.
target_compile_definitions(tally PRIVATE TALLY_VIA_PKG_CONFIG=1)

# The C library's maths is a library of its own on Unix, and part of the C runtime on
# Windows, so only the pkg-config file says to link it.
if(NOT WIN32)
  set(TALLY_LIBS_PRIVATE "-lm")
endif()
configure_file(tally.pc.in ${CMAKE_CURRENT_BINARY_DIR}/tally.pc @ONLY)

install(TARGETS tally ARCHIVE DESTINATION ${CMAKE_INSTALL_LIBDIR})
install(FILES tally.h ${CMAKE_CURRENT_BINARY_DIR}/tally_config.h
        DESTINATION ${CMAKE_INSTALL_INCLUDEDIR})
install(FILES ${CMAKE_CURRENT_BINARY_DIR}/tally.pc DESTINATION ${CMAKE_INSTALL_LIBDIR}/pkgconfig)
