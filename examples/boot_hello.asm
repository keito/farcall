; Build: nasm -f bin -Werror examples/boot_hello.asm -o examples/boot_hello.img
; Run:   cargo run --quiet -- --boot examples/boot_hello.img 2>/dev/null
; Press Ctrl-C to stop. This program stays in a loop after printing.

bits 16
cpu 8086
org 7C00h

    ; The loader places this sector at physical 7C00h. With DS=0,
    ; labels such as message refer to the correct physical addresses.
    xor ax, ax ; ax = 0
    mov ds, ax ; ds = 0
    mov es, ax ; es = 0

    ; Give BIOS interrupts a stack below the boot sector. Keep MOV SP
    ; immediately after MOV SS: x86 inhibits interrupts across this pair.
    mov ss, ax    ; ss = 0
    mov sp, 7C00h ; sp = 7C00h

    cld                 ; clear direction flag; LODSB advances SI through the message.
    mov si, message     ; si = address of first byte in message
    mov bx, 0007h       ; BH=display page 0; BL=color 7 if in graphics mode.

print_next:
    lodsb               ; set Al=[DS:SI]; since direction flag is clear, increment si
    cmp al, 0           ; set flags for `al - 0`
    je finished         ; if al was 0, jump to finished
    mov ah, 0Eh         ; BIOS teletype output: character in AL.
    int 10h
    jmp short print_next

finished:
    ; There is no DOS process to exit and no caller to return to.
    jmp short finished

message:
    db 'Hello from a boot sector!', 13, 10, 0

; A boot sector occupies 512 bytes, ending in the bytes 55h, AAh.
times 510 - ($ - $$) db 0
dw 0AA55h
