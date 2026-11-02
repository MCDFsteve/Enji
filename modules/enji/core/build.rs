fn main() {
    // Android plays through Oboe, whose C++ links the NDK's static libc++;
    // its ABI half is a library apart.
    if std::env::var("CARGO_CFG_TARGET_OS").as_deref() == Ok("android") {
        println!("cargo:rustc-link-lib=c++abi");
    }
}
