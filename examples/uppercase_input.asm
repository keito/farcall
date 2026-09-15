bits 16
cpu 8086
org 100h

    mov sp, 0200h          ; stack above this program and its buffer

    ; Process successive chunks until EOF. In a normal host terminal,
    ; press Enter to submit each line, then Ctrl-D on an empty line to finish.
read_next:
    mov bx, 0              ; stdin
    mov cx, buffer_size
    mov dx, buffer
    mov ah, 3Fh
    int 21h
    jb short io_error      ; JB also means "jump if carry"

    ; AX is in 0..64, so AL alone is enough to detect a zero-byte read.
    ; Check before entering the conversion loop or writing another chunk.
    cmp al, 0
    jnz short process_chunk
    jmp short done

process_chunk:
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
    jmp short read_next    ; reset the read arguments for every chunk

done:
    mov ax, 4C00h
    int 21h

io_error:
    mov ax, 4C01h          ; exit with code 1 on a reported DOS I/O error
    int 21h

uppercase:
    ; SI = buffer, CX = length (0..64). Changes AL, CX, SI, and flags.
    ; Checking CL is sufficient here because the buffer is smaller than 256.
    ; Accept an empty buffer too: decrementing zero would wrap to FFFF.
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
