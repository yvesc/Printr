"""Image de partage du site (docs/preview.png, 1200 × 630) : le haut de la page d'accueil, en anglais,
avec le logo réduit pour que tout tienne. Lancé par scripts/illustrations.sh :
    python partage.py URL_DU_SITE IMAGE.png
"""
import asyncio
import sys
from playwright.async_api import async_playwright

async def main():
    async with async_playwright() as p:
        b = await p.chromium.launch(executable_path="/usr/bin/chromium")
        c = await b.new_context(viewport={"width": 1200, "height": 900}, locale="en-US", color_scheme="light")
        d = await c.new_page()
        url = sys.argv[1]
        await d.goto(url)
        await d.evaluate("localStorage.setItem('printr-lang', 'en'); localStorage.setItem('printr-theme', 'light')")
        await d.goto(url)
        await d.wait_for_timeout(1500)
        await d.add_style_tag(content=".nav { display: none !important; } .hero-logo { width: 96px !important; height: auto !important; }")
        await d.wait_for_timeout(300)
        top = await d.evaluate("() => document.querySelector('.hero-logo.shot-light').getBoundingClientRect().top + scrollY")
        await d.screenshot(path=sys.argv[2], clip={"x": 0, "y": max(0, top - 20), "width": 1200, "height": 630}, full_page=True)
        await b.close()

asyncio.run(main())
