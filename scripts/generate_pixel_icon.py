from PIL import Image

def draw_retro_pet_folder_icon(palette="calico"):
    """
    32x32 master pixel canvas.
    Matches the in-game Neko / Watch-Folder retro aesthetic:
    - 62% big cute cat face & front paws
    - 38% folder slot with tab & paper corner
    - Edge-to-edge canvas usage (minimal transparent margin: 1px)
    """
    img = Image.new('RGBA', (32, 32), (0, 0, 0, 0))
    px = img.load()

    if palette == "calico":
        OUTLINE      = (42, 36, 46, 255)       # Crisp dark outline
        CAT_WHITE    = (255, 252, 248, 255)   # Cream white body
        CAT_SHADOW   = (226, 222, 232, 255)   # Soft underbelly / chin shadow
        GINGER       = (242, 134, 42, 255)    # Calico orange
        GINGER_D     = (205, 100, 28, 255)    # Ginger shadow
        GINGER_L     = (255, 172, 85, 255)    # Ginger highlight
        BLACK_PATCH  = (58, 52, 68, 255)      # Calico dark patch
        EAR_PINK     = (255, 175, 192, 255)   # Sweet ear pink
        EYE_DARK     = (30, 26, 36, 255)      # Deep eye outline/pupil
        EYE_TEAL     = (34, 156, 142, 255)    # Sparkling anime teal iris
        EYE_SHINE    = (255, 255, 255, 255)   # Bright white sparkle
        NOSE_PINK    = (246, 136, 160, 255)   # Cute nose
        BLUSH        = (255, 176, 192, 255)   # Rosy cheeks
        BELL_GOLD    = (255, 212, 48, 255)    # Collar bell
        COLLAR_RED   = (224, 46, 46, 255)     # Red ribbon collar
        
        # Golden folder slot matching in-app assets
        FOLDER_OUT   = (48, 38, 30, 255)      # Folder boundary
        FOLDER_BODY  = (250, 196, 56, 255)    # Main folder face
        FOLDER_LIGHT = (255, 224, 105, 255)   # Top rim highlight
        FOLDER_DARK  = (214, 152, 32, 255)    # Bottom shadow
        FOLDER_BACK  = (198, 136, 24, 255)    # Folder interior / back flap
        TAB_COLOR    = (245, 182, 44, 255)    # Folder tab
        
        # Little file paper peeking
        PAPER_WHITE  = (248, 250, 255, 255)
        PAPER_BLUE   = (65, 130, 245, 255)
        PAPER_LINE   = (175, 185, 205, 255)
    else: # Clean monochrome / Game Boy B&W
        OUTLINE      = (20, 20, 24, 255)
        CAT_WHITE    = (255, 255, 255, 255)
        CAT_SHADOW   = (220, 222, 228, 255)
        GINGER       = (255, 255, 255, 255)
        GINGER_D     = (220, 222, 228, 255)
        GINGER_L     = (255, 255, 255, 255)
        BLACK_PATCH  = (40, 40, 48, 255)
        EAR_PINK     = (210, 212, 220, 255)
        EYE_DARK     = (20, 20, 24, 255)
        EYE_TEAL     = (30, 30, 36, 255)
        EYE_SHINE    = (255, 255, 255, 255)
        NOSE_PINK    = (40, 40, 48, 255)
        BLUSH        = (230, 232, 238, 255)
        BELL_GOLD    = (255, 255, 255, 255)
        COLLAR_RED   = (50, 50, 60, 255)
        
        FOLDER_OUT   = (20, 20, 24, 255)
        FOLDER_BODY  = (255, 255, 255, 255)
        FOLDER_LIGHT = (255, 255, 255, 255)
        FOLDER_DARK  = (195, 198, 205, 255)
        FOLDER_BACK  = (160, 164, 172, 255)
        TAB_COLOR    = (225, 228, 235, 255)
        
        PAPER_WHITE  = (255, 255, 255, 255)
        PAPER_BLUE   = (60, 60, 70, 255)
        PAPER_LINE   = (180, 184, 192, 255)

    def set_p(x, y, color):
        if 0 <= x < 32 and 0 <= y < 32:
            px[x, y] = color

    def hline(x0, x1, y, color):
        for x in range(x0, x1 + 1):
            set_p(x, y, color)

    def vline(x, y0, y1, color):
        for y in range(y0, y1 + 1):
            set_p(x, y, color)

    def rect(x0, y0, x1, y1, color):
        for y in range(y0, y1 + 1):
            for x in range(x0, x1 + 1):
                set_p(x, y, color)

    # -------------------------------------------------------------
    # LAYER 1: FOLDER BACK & TAB (behind cat)
    # -------------------------------------------------------------
    # Tab at top-left: x=2..11, y=14..18
    hline(4, 10, 14, FOLDER_OUT)
    set_p(3, 15, FOLDER_OUT); set_p(11, 15, FOLDER_OUT)
    rect(4, 15, 10, 17, TAB_COLOR)
    set_p(3, 16, TAB_COLOR); set_p(3, 17, TAB_COLOR)
    set_p(11, 16, TAB_COLOR)

    # Back interior wall of folder
    hline(2, 29, 17, FOLDER_OUT)
    rect(2, 18, 29, 20, FOLDER_BACK)

    # Mini file paper peeking behind the cat on the right side!
    # x: 23..28, y: 13..18
    hline(24, 27, 12, FOLDER_OUT)
    vline(23, 13, 18, FOLDER_OUT); vline(28, 13, 18, FOLDER_OUT)
    rect(24, 13, 27, 18, PAPER_WHITE)
    # Blue document header on the peeking file
    hline(24, 27, 13, PAPER_BLUE)
    # Doc text line
    hline(24, 26, 15, PAPER_LINE)

    # -------------------------------------------------------------
    # LAYER 2: CAT HEAD, EARS, AND FACE (Rows 2..18)
    # -------------------------------------------------------------
    # Left Ear (White) - tip at (8, 2)
    set_p(8, 2, OUTLINE)
    set_p(7, 3, OUTLINE); set_p(9, 3, OUTLINE)
    set_p(6, 4, OUTLINE); set_p(10, 4, OUTLINE)
    set_p(5, 5, OUTLINE); set_p(11, 5, OUTLINE)
    set_p(5, 6, OUTLINE); set_p(12, 6, OUTLINE)
    
    # Left Ear Fill
    set_p(8, 3, CAT_WHITE)
    set_p(7, 4, CAT_WHITE); set_p(8, 4, EAR_PINK); set_p(9, 4, CAT_WHITE)
    set_p(6, 5, CAT_WHITE); set_p(7, 5, EAR_PINK); set_p(8, 5, EAR_PINK); set_p(9, 5, EAR_PINK); set_p(10, 5, CAT_WHITE)
    set_p(6, 6, CAT_WHITE); set_p(7, 6, EAR_PINK); set_p(8, 6, EAR_PINK); set_p(9, 6, EAR_PINK); set_p(10, 6, EAR_PINK); set_p(11, 6, CAT_WHITE)

    # Right Ear (Ginger calico) - tip at (23, 2)
    set_p(23, 2, OUTLINE)
    set_p(22, 3, OUTLINE); set_p(24, 3, OUTLINE)
    set_p(21, 4, OUTLINE); set_p(25, 4, OUTLINE)
    set_p(20, 5, OUTLINE); set_p(26, 5, OUTLINE)
    set_p(19, 6, OUTLINE); set_p(26, 6, OUTLINE)

    # Right Ear Fill
    set_p(23, 3, GINGER)
    set_p(22, 4, GINGER); set_p(23, 4, EAR_PINK); set_p(24, 4, GINGER)
    set_p(21, 5, GINGER); set_p(22, 5, EAR_PINK); set_p(23, 5, EAR_PINK); set_p(24, 5, GINGER); set_p(25, 5, GINGER)
    set_p(20, 6, GINGER); set_p(21, 6, EAR_PINK); set_p(22, 6, EAR_PINK); set_p(23, 6, EAR_PINK); set_p(24, 6, GINGER); set_p(25, 6, GINGER)

    # Head Crown (between ears, y=6..7)
    hline(13, 18, 6, OUTLINE)
    hline(12, 19, 7, CAT_WHITE)

    # Head Outer Sides Outline (y=7..17)
    vline(4, 7, 16, OUTLINE)
    vline(27, 7, 16, OUTLINE)
    set_p(5, 17, OUTLINE); set_p(26, 17, OUTLINE)

    # Head Base Fill (Cream white)
    rect(5, 7, 26, 16, CAT_WHITE)

    # --- Calico Head Markings ---
    # Ginger Calico patch on right cheek & forehead
    rect(20, 7, 26, 11, GINGER)
    rect(22, 12, 26, 14, GINGER)
    set_p(21, 12, GINGER); set_p(19, 8, GINGER_L); set_p(20, 8, GINGER_L)
    set_p(26, 15, GINGER); set_p(25, 15, GINGER)

    # Black Patch on left ear base & temple
    rect(5, 7, 9, 10, BLACK_PATCH)
    set_p(10, 8, BLACK_PATCH); set_p(10, 9, BLACK_PATCH)
    rect(5, 11, 7, 13, BLACK_PATCH)

    # Lower face soft shadow
    hline(6, 10, 16, CAT_SHADOW)
    hline(21, 25, 16, CAT_SHADOW)

    # --- Cute Face Features ---
    # Big sparkling round anime cat eyes (3x4 pixels)
    # Left eye at x: 8..10, y: 10..13
    rect(8, 10, 10, 13, EYE_DARK)
    set_p(8, 11, EYE_TEAL); set_p(9, 11, EYE_TEAL); set_p(8, 12, EYE_TEAL); set_p(9, 12, EYE_TEAL)
    set_p(8, 10, EYE_SHINE); set_p(9, 10, EYE_SHINE) # Big round eye shine!
    set_p(8, 11, EYE_SHINE)

    # Right eye at x: 21..23, y: 10..13
    rect(21, 10, 23, 13, EYE_DARK)
    set_p(21, 11, EYE_TEAL); set_p(22, 11, EYE_TEAL); set_p(21, 12, EYE_TEAL); set_p(22, 12, EYE_TEAL)
    set_p(21, 10, EYE_SHINE); set_p(22, 10, EYE_SHINE) # Big round eye shine!
    set_p(21, 11, EYE_SHINE)

    # Cute tiny pink button nose (x: 15..16, y: 12)
    set_p(15, 12, NOSE_PINK); set_p(16, 12, NOSE_PINK)

    # Cute feline smile (:3 / w mouth)
    set_p(14, 14, OUTLINE); set_p(15, 13, OUTLINE); set_p(16, 13, OUTLINE); set_p(17, 14, OUTLINE)
    set_p(13, 13, OUTLINE); set_p(18, 13, OUTLINE)

    # Rosy blushing cheeks
    rect(5, 13, 7, 14, BLUSH)
    rect(24, 13, 26, 14, BLUSH)

    # Cute cat whiskers (subtle 2px strokes)
    set_p(2, 11, OUTLINE); set_p(3, 11, OUTLINE)
    set_p(2, 14, OUTLINE); set_p(3, 14, OUTLINE)
    set_p(28, 11, OUTLINE); set_p(29, 11, OUTLINE)
    set_p(28, 14, OUTLINE); set_p(29, 14, OUTLINE)

    # Tiny golden bell peeking between paws!
    set_p(15, 17, COLLAR_RED); set_p(16, 17, COLLAR_RED)
    rect(15, 18, 16, 19, BELL_GOLD)
    set_p(15, 18, EYE_SHINE)

    # -------------------------------------------------------------
    # LAYER 3: FOLDER BODY & RIM (Rows 18..31, x: 1..30)
    # -------------------------------------------------------------
    # Top rim line of front folder cover
    # Cut out spaces for paws at x: 9..13 and x: 18..22
    hline(1, 8, 18, FOLDER_OUT)
    hline(14, 17, 18, FOLDER_OUT)
    hline(23, 30, 18, FOLDER_OUT)

    # Top rim highlight
    hline(2, 8, 19, FOLDER_LIGHT)
    hline(14, 17, 19, FOLDER_LIGHT)
    hline(23, 29, 19, FOLDER_LIGHT)

    # Main folder face fill
    rect(2, 20, 29, 28, FOLDER_BODY)
    # Bottom shading & bevel
    hline(2, 29, 29, FOLDER_DARK)
    hline(3, 28, 30, FOLDER_DARK)

    # Folder outer border
    vline(1, 19, 29, FOLDER_OUT)   # Left border
    vline(30, 19, 29, FOLDER_OUT)  # Right border
    set_p(2, 30, FOLDER_OUT); set_p(29, 30, FOLDER_OUT) # Rounded corners
    hline(3, 28, 31, FOLDER_OUT)   # Bottom baseline

    # Stylized folder tab detail / line on folder face
    for i in range(10):
        set_p(19 + i, 23 + (i // 3), FOLDER_DARK)

    # -------------------------------------------------------------
    # LAYER 4: CHUBBY FRONT PAWS (Gripping the folder rim!)
    # Left paw (x: 9..13, y: 17..21)
    # Right paw (x: 18..22, y: 17..21)
    # -------------------------------------------------------------
    for px_start in [9, 18]:
        # Top rounded curve of paw
        hline(px_start + 1, px_start + 3, 16, OUTLINE)
        set_p(px_start, 17, OUTLINE); set_p(px_start + 4, 17, OUTLINE)
        vline(px_start - 1, 18, 20, OUTLINE)
        vline(px_start + 5, 18, 20, OUTLINE)
        hline(px_start, px_start + 4, 21, OUTLINE)
        
        # Paw fill
        rect(px_start, 17, px_start + 4, 20, CAT_WHITE)
        # Toe divisions (cute little bean lines)
        vline(px_start + 1, 18, 20, CAT_SHADOW)
        vline(px_start + 3, 18, 20, CAT_SHADOW)
        set_p(px_start + 2, 17, CAT_WHITE)

    return img

if __name__ == "__main__":
    calico = draw_retro_pet_folder_icon("calico")
    calico.save("assets/icon_retro_calico.png")
    
    mono = draw_retro_pet_folder_icon("mono")
    mono.save("assets/icon_retro_mono.png")

    for name, icon in [("calico", calico), ("mono", mono)]:
        # App high-res icons (nearest neighbor preserves authentic pixel art)
        icon.resize((256, 256), Image.NEAREST).save(f"assets/icon_retro_{name}_256.png")
        icon.resize((512, 512), Image.NEAREST).save(f"assets/icon_retro_{name}_512.png")
        
        # Real-world system tray scale previews
        for s in [16, 24, 32, 48]:
            icon.resize((s, s), Image.NEAREST).save(f"scratch_retro_{name}_{s}.png")

    print("Successfully generated retro companion icons!")
