from PIL import Image, ImageDraw

def create_file_icon():
    doc = Image.new('RGBA', (32, 32), (0, 0, 0, 0))
    draw = ImageDraw.Draw(doc)
    # Outline
    draw.rectangle([7, 4, 24, 27], fill=(30, 35, 45, 255))
    # Paper body
    draw.rectangle([8, 5, 23, 26], fill=(245, 248, 255, 255))
    # Folded flap
    draw.polygon([(18, 5), (23, 10), (18, 10)], fill=(195, 205, 220, 255))
    draw.line([(18, 5), (18, 10), (23, 10)], fill=(30, 35, 45, 255))
    # Color header
    draw.rectangle([10, 8, 16, 10], fill=(70, 130, 240, 255))
    # Document content lines
    draw.rectangle([10, 13, 21, 14], fill=(150, 165, 180, 255))
    draw.rectangle([10, 17, 21, 18], fill=(150, 165, 180, 255))
    draw.rectangle([10, 21, 18, 22], fill=(150, 165, 180, 255))
    doc.save('assets/pet/file_icon.png')
    print("Created assets/pet/file_icon.png")

def create_zzz_particles():
    # 4 frames of floating Zzz (32x32 each -> 128x32 strip)
    sheet = Image.new('RGBA', (128, 32), (0, 0, 0, 0))
    draw = ImageDraw.Draw(sheet)

    def draw_z(ox, oy, size, color):
        # Draw pixel 'Z'
        if size == 1: # Small 'z' (4x4)
            draw.rectangle([ox, oy, ox+3, oy], fill=color)
            draw.point([(ox+2, oy+1), (ox+1, oy+2)], fill=color)
            draw.rectangle([ox, oy+3, ox+3, oy+3], fill=color)
        else: # Large 'Z' (6x6)
            draw.rectangle([ox, oy, ox+5, oy], fill=color)
            draw.point([(ox+4, oy+1), (ox+3, oy+2), (ox+2, oy+3), (ox+1, oy+4)], fill=color)
            draw.rectangle([ox, oy+5, ox+5, oy+5], fill=color)

    c1 = (120, 180, 255, 200)
    c2 = (140, 200, 255, 240)
    c3 = (160, 220, 255, 255)

    # Frame 0: small z near bottom
    draw_z(14, 22, 1, c1)
    # Frame 1: small z floating up, medium Z appears
    draw_z(12, 16, 1, c2)
    draw_z(18, 22, 1, c1)
    # Frame 2: big Z rising
    draw_z(32 + 10, 10, 1, c1)
    draw_z(32 + 16, 16, 2, c2)
    # Frame 3: top Z fading out, new small z at bottom
    draw_z(64 + 8, 5, 2, (160, 220, 255, 140))
    draw_z(64 + 18, 14, 1, c2)
    draw_z(64 + 14, 23, 1, c1)
    # Frame 4:
    draw_z(96 + 12, 8, 2, c2)
    draw_z(96 + 18, 18, 1, c1)

    sheet.save('assets/pet/zzz_particles.png')
    print("Created assets/pet/zzz_particles.png")

if __name__ == "__main__":
    create_file_icon()
    create_zzz_particles()
