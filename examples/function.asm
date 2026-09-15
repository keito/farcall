bits 16
cpu 8086
org 100h

    ; Stack offsets fit in the emulator's current 512-byte memory.
    ; For this small program, the stack stays above the code and data.
    mov sp, 0200h

    ; Distinct values make incorrect push/pop order visible in the trace.
    mov al, 34h
    mov ah, 12h
    mov bx, 5678h
    mov cx, msg_len
    mov dx, msg
    call print_message

    ; After returning: AX = 1234h, BX = 5678h, SP = 0200h.
    mov al, 0
    mov ah, 4Ch
    int 21h

print_message:
    push ax            ; save the caller's registers
    push bx
    mov bx, 1          ; stdout
    mov ah, 40h        ; DOS write service
    call write_stdout  ; nested call: another return address goes on the stack
    pop bx             ; restore in reverse order
    pop ax
    ret

write_stdout:
    ; On entry: SP = 01F8h (two calls and two saved words).
    int 21h            ; writes msg; returns the byte count in AX
    ret

msg:
    db "calls work"
msg_len equ $ - msg
