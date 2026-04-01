fn main() {
    use std::{env, fs, path::PathBuf};

    println!("cargo:rerun-if-changed=build.rs");
    println!("cargo:rerun-if-env-changed=LOG");
    println!("cargo:rerun-if-env-changed=BASE_ADDRESS");
    println!("cargo:rerun-if-env-changed=CHAPTER");
    println!("cargo:rerun-if-env-changed=T2L4_SCENARIO");
    println!("cargo:rerun-if-env-changed=T2L5_SCENARIO");

    if let Ok(chapter) = env::var("CHAPTER") {
        println!("cargo:rustc-env=CHAPTER={chapter}");
    }
    if let Ok(scenario) = env::var("T2L4_SCENARIO") {
        println!("cargo:rustc-env=T2L4_SCENARIO={scenario}");
    }
    if let Ok(scenario) = env::var("T2L5_SCENARIO") {
        println!("cargo:rustc-env=T2L5_SCENARIO={scenario}");
    }

    if let Some(base) = env::var("BASE_ADDRESS")
        .ok()
        .and_then(|s| s.parse::<u64>().ok())
    {
        let text = format!(
            "\
OUTPUT_ARCH(riscv)
ENTRY(_start)
SECTIONS {{
    . = {base};
    .text : {{
        *(.text.entry)
        *(.text .text.*)
    }}
    .rodata : {{
        *(.rodata .rodata.*)
        *(.srodata .srodata.*)
    }}
    .data : {{
        *(.data .data.*)
        *(.sdata .sdata.*)
    }}
    .bss : {{
        *(.bss .bss.*)
        *(.sbss .sbss.*)
    }}
}}"
        );
        let ld = PathBuf::from(env::var_os("OUT_DIR").unwrap()).join("linker.ld");
        fs::write(&ld, text).unwrap();
        println!("cargo:rustc-link-arg=-T{}", ld.display());
    }
}
