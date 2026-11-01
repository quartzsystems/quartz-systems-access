# Read by CMake through CMAKE_TOOLCHAIN_FILE (set in .cargo/config.toml) when
# fltk-sys builds FLTK. cfltk already uses the static MSVC runtime, but FLTK's
# own option defaults to the runtime DLL; force it off so the whole binary
# links one static CRT. On other platforms FLTK drops the option.
set(FLTK_MSVC_RUNTIME_DLL OFF CACHE BOOL "Use the MSVC runtime DLL" FORCE)
