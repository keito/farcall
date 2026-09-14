org 100h

    mov bx, 1          ; stdout
    mov cx, 3          ; initial count
    mov dx, msg

again:
    mov ah, 40h        ; reset service ID: the write overwrites AX
    int 21h            ; write CX bytes from msg

    dec cx
    jnz short again

    mov al, 0          ; exit successfully
    mov ah, 4Ch
    int 21h

msg:
    db "abc"
