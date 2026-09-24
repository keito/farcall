; Build: nasm -f bin -Werror examples/cga_graphics_hello.asm -o examples/cga_graphics_hello.com
; Run:   cargo run -- --com examples/cga_graphics_hello.com
; Expected: HELLO WORLD in white on black, centered in 320x200 graphics.
; Press any key to restore 80-column text mode and exit to DOS.
;
; Requires BIOS INT 10h mode 04h, CGA graphics rendering, and port 3D9h.
; The letters are our own bitmap: no BIOS graphics character service needed.
; CGA layout reference: IBM Color/Graphics Monitor Adapter, graphics modes:
; https://minuszerodegrees.net/oa/OA%20-%20IBM%20Color%20Graphics%20Monitor%20Adapter%20(CGA).pdf

bits 16
cpu 8086
org 100h

    mov ax, 0004h       ; BIOS: 320x200, four colors.
    int 10h

    ; CGA color select: palette 1, high intensity, black background.
    ; Pixel values 0/1/2/3 become black/light cyan/light magenta/white.
    mov dx, 03D9h
    mov al, 30h
    out dx, al

    push cs
    pop ds
    cld
    mov ax, 0B800h
    mov es, ax          ; ES:DI addresses CGA video memory.
    xor di, di
    xor ax, ax
    mov cx, 2000h      ; Clear all 16 KiB, including both scanline banks.
    rep stosw

    ; Four pixels per byte, most significant pair first.
    ; offset(x,y) = (y & 1)*2000h + (y/2)*80 + x/4
    ; Our 88x8 bitmap begins at x=116, y=96.
    mov di, (96 / 2) * 80 + (116 / 4)
    mov si, message_bitmap
    mov bp, 8          ; Eight scanlines in the bitmap.

draw_row:
    mov dx, 11         ; Eleven eight-pixel chunks per scanline.
draw_chunk:
    lodsb              ; Read eight monochrome pixels, leftmost in bit 7.
    xor bx, bx
    mov cx, 8
expand_pixel:
    shl bx, 1
    shl bx, 1          ; Make room for the next two-bit CGA pixel.
    shl al, 1          ; Move the next bitmap bit into Carry.
    jnc black_pixel
    or bl, 3           ; A set bitmap bit becomes color 3 (white).
black_pixel:
    loop expand_pixel

    ; BX now holds eight pixels. Store the LEFT four first (BH).
    ; STOSW would reverse these bytes on the little-endian 8088.
    mov al, bh
    stosb
    mov al, bl
    stosb
    dec dx
    jnz draw_chunk

    sub di, 22         ; Return to the start of this scanline.
    xor di, 2000h      ; Alternate between even and odd scanline banks.
    test di, 2000h
    jnz next_row       ; Even -> odd: same offset within the bank.
    add di, 80         ; Odd -> even: advance one row within the bank.
next_row:
    dec bp
    jnz draw_row

    xor ah, ah         ; BIOS: wait for a key (keeps the image visible).
    int 16h
    mov ax, 0003h      ; BIOS: restore 80x25 text mode.
    int 10h
    mov ax, 4C00h      ; DOS: exit successfully.
    int 21h

; Original 8x8 letter shapes, stored by scanline, not by character.
; Each byte describes one letter's row; 1=white, 0=black.
;       H    E    L    L    O   space  W    O    R    L    D
message_bitmap:
    db 66h, 7Eh, 60h, 60h, 3Ch, 00h, 66h, 3Ch, 7Ch, 60h, 78h
    db 66h, 60h, 60h, 60h, 66h, 00h, 66h, 66h, 66h, 60h, 6Ch
    db 66h, 60h, 60h, 60h, 66h, 00h, 66h, 66h, 66h, 60h, 66h
    db 7Eh, 7Ch, 60h, 60h, 66h, 00h, 66h, 66h, 7Ch, 60h, 66h
    db 66h, 60h, 60h, 60h, 66h, 00h, 7Eh, 66h, 6Ch, 60h, 66h
    db 66h, 60h, 60h, 60h, 66h, 00h, 7Eh, 66h, 66h, 60h, 6Ch
    db 66h, 7Eh, 7Eh, 7Eh, 3Ch, 00h, 66h, 3Ch, 66h, 7Eh, 78h
    db 00h, 00h, 00h, 00h, 00h, 00h, 00h, 00h, 00h, 00h, 00h
