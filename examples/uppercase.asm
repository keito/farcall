bits 16
cpu 8086
org 100h

    ; This first version assumes every character is lowercase ASCII.
    mov si, msg        ; address of the current character
    mov cx, msg_len    ; number of characters remaining (must be nonzero)

again:
    mov al, [si]       ; load a character from guest memory
    sub al, 20h        ; lowercase ASCII -> uppercase ASCII
    mov [si], al       ; store the transformed character back in memory
    inc si            ; advance to the next character
    dec cx
    jnz short again

    ; Print the modified buffer. CX is zero after the loop, so reset it.
    mov bx, 1          ; stdout
    mov cx, msg_len
    mov dx, msg        ; SI now points past the string; use its start address
    mov ah, 40h
    int 21h

    mov al, 0          ; exit successfully
    mov ah, 4Ch
    int 21h

msg:
    db "hello"
msg_len equ $ - msg
