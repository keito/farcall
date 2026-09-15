bits 16
cpu 8086
org 100h

    mov sp, 0200h          ; stack above this program and its buffer

    ; Type a short line and press Enter. Process one read of up to 64 bytes.
    mov bx, 0              ; stdin
    mov cx, buffer_size
    mov dx, buffer
    mov ah, 3Fh
    int 21h
    jb short io_error      ; JB also means "jump if carry"

    push ax                ; save the actual byte count for printing
    mov cl, al             ; copy AX to CX using supported byte MOVs
    mov ch, ah
    mov si, buffer
    call uppercase
    pop ax                 ; conversion used up CX and changed AL

    mov cl, al             ; print exactly the original byte count
    mov ch, ah
    mov bx, 1              ; stdout
    mov dx, buffer
    mov ah, 40h
    int 21h
    jb short io_error

    mov ax, 4C00h
    int 21h

io_error:
    mov ax, 4C01h          ; exit with code 1 on a reported DOS I/O error
    int 21h

uppercase:
    ; SI = buffer, CX = length (0..64). Changes AL, CX, SI, and flags.
    ; Checking CL is sufficient here because the buffer is smaller than 256.
    ; Return immediately on EOF: decrementing a zero count would wrap to FFFF.
    mov al, cl
    cmp al, 0
    jnz short next_char
    ret

next_char:
    mov al, [si]
    cmp al, 'a'
    jb short advance       ; spaces, digits, uppercase letters, etc.
    cmp al, 'z'
    ja short advance       ; punctuation and bytes above lowercase ASCII
    sub al, 20h
    mov [si], al

advance:
    inc si
    dec cx
    jnz short next_char
    ret

buffer_size equ 64
buffer:
    times buffer_size db 0
