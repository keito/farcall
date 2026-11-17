# Farcall, an IBM PC emulator

Farcall is an IBM PC (8088) emulator, written for educational purposes. [Website](https://www.dialup.net/farcall/)

It currently has support for running COM files, booting from floppy images, and using a BIOS ROM.

Example invocation:

```sh
cargo run --release --features terminal,window -- \
    --floppy-a FLOPPY_A_IMAGE --floppy-a-type 720k \
    --floppy-b FLOPPY_B_IMAGE --floppy-b-type 720k \
    --rom BIOS_ROM_IMAGE \
    --frontend terminal-graphics
```

See `--help` for a list of available options.

## Cargo features
* terminal - Adds `terminal-text` (text-only stdout) and `terminal-graphics` (Kitty graphics stdout) frontends
* window (default) - Adds `window` (GUI) frontend

## Acknowledgements
The MIT license covers Farcall's own code.

* `CGA.F08: IBM CGA 8x8 character font` is from VileR's [vga-text-mode-fonts](https://github.com/viler-int10h/vga-text-mode-fonts/blob/master/FONTS/PC-IBM/CGA.F08)
