; Build: nasm -f bin -Werror examples/boot_two_stage.asm -o examples/boot_two_stage.img
; Run:   cargo run --quiet -- --boot examples/boot_two_stage.img 2>/dev/null
; Press Ctrl-C to stop after the messages appear.
;
; Image bytes 0..511:    sector 1, loaded by the emulator at 0000:7C00h.
; Image bytes 512..1023: sector 2, loaded by this program at 0000:7E00h.

bits 16
cpu 8086
org 7C00h

stage1:
    xor ax, ax
    mov ds, ax
    mov es, ax
    mov ss, ax
    mov sp, 7C00h       ; Keep this immediately after MOV SS.
    cld

    mov si, loading_message
    call print_string

    ; Ask the BIOS to copy sector 2 into RAM immediately above sector 1.
    mov ax, 0201h       ; AH=02h: read; AL=1: one sector.
    mov bx, stage2      ; ES:BX = 0000:7E00h, destination buffer.
    mov cx, 0002h       ; CH=0: cylinder; CL=2: sector (numbered from 1).
    mov dx, 0000h       ; DH=0: head; DL=0: first floppy drive.
    int 13h
    jc disk_error       ; Test the BIOS result before anything changes flags.

    ; This code was not in RAM until the disk read above succeeded.
    jmp near stage2

disk_error:
    mov si, error_message
    call print_string

finished:
    jmp short finished  ; There is no DOS to exit to.

; Print a zero-terminated string at DS:SI using BIOS services only.
; Changes AX, BX, SI, and flags. Both stages can call this routine:
; the first sector remains in RAM after the second sector is loaded.
print_string:
    mov bx, 0007h       ; Display page 0; color 7 for graphics modes.
.next:
    lodsb
    cmp al, 0
    je .done
    mov ah, 0Eh
    int 10h
    jmp short .next
.done:
    ret

loading_message:
    db 'Stage 1: reading sector 2...', 13, 10, 0
error_message:
    db 'Disk read failed.', 13, 10, 0

times 510 - ($ - $$) db 0
dw 0AA55h              ; Boot signature belongs at the end of sector 1.

; Exactly 512 bytes into the image. With ORG 7C00h, this label is 7E00h.
stage2:
    mov si, stage2_message
    call print_string
    jmp near finished

stage2_message:
    db 'Stage 2: loaded from disk and running!', 13, 10, 0

times 1024 - ($ - $$) db 0
