bits 16
cpu 8086
org 100h

    ; Read up to 64 bytes from stdin into guest memory.
    ; With a normal host terminal, type a short line and press Enter.
    mov bx, 0              ; stdin
    mov cx, buffer_size
    mov dx, buffer
    mov ah, 3Fh            ; DOS read service
    int 21h

    ; AX is the actual byte count, which may be less than buffer_size.
    ; Copy AX into CX using the byte MOV instructions already implemented.
    ; Do this before changing AH to select the write service.
    mov cl, al
    mov ch, ah
    mov bx, 1              ; stdout; DX still points to the buffer
    mov ah, 40h            ; DOS write service
    int 21h

    mov ax, 4C00h          ; exit successfully
    int 21h

    ; This starter assumes successful DOS reads/writes. Add carry-flag
    ; error branches when the emulator supports those conditional jumps.
buffer_size equ 64
buffer:
    times buffer_size db 0
