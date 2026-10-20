import os
from PIL import Image, ImageDraw

def create_aerospace_icon(size=512):
    # Create image with transparent background
    img = Image.new('RGBA', (size, size), (0, 0, 0, 0))
    draw = ImageDraw.Draw(img)
    
    # Outer circular badge background with subtle gradient look
    margin = size * 0.05
    # Dark navy aerospace circle
    draw.ellipse([margin, margin, size - margin, size - margin], fill=(10, 18, 36, 255), outline=(56, 189, 248, 255), width=int(size * 0.035))
    
    # Inner glow circle
    inner_m = margin + size * 0.04
    draw.ellipse([inner_m, inner_m, size - inner_m, size - inner_m], outline=(14, 116, 144, 180), width=int(size * 0.015))
    
    # Supersonic Mach cone / Shock wave angle (cyan dashed / thin line)
    cone_tip = (size * 0.78, size * 0.5)
    draw.line([(size * 0.2, size * 0.18), cone_tip], fill=(56, 189, 248, 160), width=int(size * 0.02))
    draw.line([(size * 0.2, size * 0.82), cone_tip], fill=(56, 189, 248, 160), width=int(size * 0.02))
    
    # Airfoil profile shape (swept supersonic delta / high-lift foil)
    # Wing points
    nose = (size * 0.82, size * 0.5)
    top_camber = (size * 0.48, size * 0.32)
    le_top = (size * 0.65, size * 0.38)
    te = (size * 0.18, size * 0.5)
    bottom_camber = (size * 0.48, size * 0.56)
    
    # Main wing polygon
    wing_pts = [
        nose,
        le_top,
        top_camber,
        (size * 0.30, size * 0.44),
        te,
        (size * 0.30, size * 0.52),
        bottom_camber,
        (size * 0.68, size * 0.54),
        nose
    ]
    draw.polygon(wing_pts, fill=(56, 189, 248, 255), outline=(255, 255, 255, 230))
    
    # Center chord line (amber / gold glow)
    draw.line([(size * 0.22, size * 0.5), (size * 0.78, size * 0.5)], fill=(251, 191, 36, 220), width=int(size * 0.025))
    
    # Streamlines
    for y_offset in [-size * 0.22, -size * 0.12, size * 0.12, size * 0.22]:
        sy = size * 0.5 + y_offset
        draw.arc([size * 0.1, sy - size * 0.08, size * 0.9, sy + size * 0.08], start=180, end=0, fill=(125, 211, 252, 120), width=int(size * 0.015))
        
    return img

os.makedirs('/home/claude/GLENN_extended/build', exist_ok=True)
icon_512 = create_aerospace_icon(512)
icon_512.save('/home/claude/GLENN_extended/build/icon.png', 'PNG')

# Generate multi-size Windows .ico
sizes = [(16, 16), (32, 32), (48, 48), (64, 64), (128, 128), (256, 256)]
icon_512.save('/home/claude/GLENN_extended/build/icon.ico', format='ICO', sizes=sizes)
print("Icons generated successfully in build/icon.png and build/icon.ico")
