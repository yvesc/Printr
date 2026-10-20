"""Captures d'écran de l'interface web pour le site et le README (données fictives).

Lancé par photo-session.sh, qui démarre un Printr temporaire : python capture.py URL DOSSIER
"""

import asyncio
import os
import sys

from playwright.async_api import async_playwright

URL, OUT = sys.argv[1], sys.argv[2]
PHOTO = sys.argv[3]
# Textes de l'interface à cliquer ou à saisir, dans la langue du Printr temporaire (PRINTR_LANG).
EN = os.environ.get("PRINTR_LANG", "fr").startswith("en")
W = {
    "enter": "Sign in" if EN else "Entrer",
    "my_tickets": "My tickets" if EN else "Mes tickets",
    "edit": "Edit" if EN else "Modifier",
    "preview": "Preview" if EN else "Aperçu",
    "add_block": "Add a block" if EN else "Ajouter un bloc",
    "games": "Game break" if EN else "Pause jeux",
    "note": "A little coffee to start the day right. Thinking of you!" if EN
            else "Un petit café pour bien commencer la journée. Je pense fort à toi !",
    "locale": "en-US" if EN else "fr-FR",
}


async def snap(page, path):
    """Capture d'écran, annoncée sur la sortie (scripts/illustrations.sh affiche la progression)."""
    await page.screenshot(path=path)
    print(path, flush=True)


async def login(page, name, password="demo1234"):
    await page.goto(URL)
    await page.get_by_text(name, exact=True).first.click()
    await page.fill("input[type=password]", password)
    await page.click(f"button:has-text('{W['enter']}')")
    await page.wait_for_selector(f"text={W['my_tickets']}")


async def open_preset(page, name):
    card = page.locator(".preset", has_text=name)
    await card.get_by_role("button", name=W["edit"]).click()
    await page.wait_for_timeout(1500)


async def mobile(p):
    browser = await p.chromium.launch(executable_path="/usr/bin/chromium")
    ctx = await browser.new_context(viewport={"width": 390, "height": 844}, device_scale_factor=2,
                                    is_mobile=True, has_touch=True, locale=W["locale"], color_scheme="light")
    page = await ctx.new_page()
    await page.goto(URL)
    await page.wait_for_timeout(500)
    await page.get_by_text("Alex", exact=True).first.click()
    await page.wait_for_timeout(300)
    await snap(page, f"{OUT}/mobile-login.png")
    await login(page, "Alex")
    await page.wait_for_timeout(600)
    await snap(page, f"{OUT}/mobile-home.png")

    await open_preset(page, W["games"])
    await page.get_by_role("button", name=W["preview"]).first.click()
    await page.wait_for_timeout(1500)
    await snap(page, f"{OUT}/mobile-preview.png")
    await page.keyboard.press("Escape")

    await page.goto(URL + "/#/compose")
    await page.wait_for_timeout(400)
    await page.locator(".block-head").nth(1).click()
    await page.wait_for_timeout(400)
    await snap(page, f"{OUT}/mobile-compose.png")

    await page.goto(URL + "/#/message")
    await page.set_input_files("input[type=file]", PHOTO)
    await page.wait_for_timeout(1500)
    await page.fill("textarea", W["note"])
    await page.wait_for_timeout(1500)
    await snap(page, f"{OUT}/mobile-message.png")
    await browser.close()


async def desktop(p, scheme):
    browser = await p.chromium.launch(executable_path="/usr/bin/chromium")
    ctx = await browser.new_context(viewport={"width": 1440, "height": 900}, device_scale_factor=2,
                                    locale=W["locale"], color_scheme=scheme)
    page = await ctx.new_page()
    await login(page, "Alex")
    await page.wait_for_timeout(600)
    await snap(page, f"{OUT}/desktop-home-{scheme}.png")
    await open_preset(page, W["games"])
    await snap(page, f"{OUT}/desktop-compose-{scheme}.png")
    await page.click(f"text={W['add_block']}")
    await page.wait_for_timeout(600)
    await snap(page, f"{OUT}/desktop-catalog-{scheme}.png")
    await browser.close()


async def main():
    async with async_playwright() as p:
        await mobile(p)
        await desktop(p, "light")
        await desktop(p, "dark")


asyncio.run(main())
