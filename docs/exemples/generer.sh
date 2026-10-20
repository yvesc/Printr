#!/usr/bin/env bash
# Régénère les exemples de blocs (docs/exemples/*.png) dans l'émulateur, avec des données
# fictives uniquement (liste de courses et historique de Camille, Alex et Léo).
# Usage : docs/exemples/generer.sh [--en] [bloc…]   (défaut : tous les .json du dossier)
#   --en : tickets en anglais (PRINTR_LANG=en) dans docs/exemples/en/, à partir de en/<bloc>.json
#          s'il existe (textes libres traduits), sinon du même exemple que le français.
# Prérequis : uv et l'émulateur (uvx emupos).
set -euo pipefail
cd "$(dirname "$0")/../.."

OUT=docs/exemples
if [ "${1:-}" = "--en" ]; then
  shift
  OUT=docs/exemples/en
  export PRINTR_LANG=en
else
  export PRINTR_LANG=fr
fi

PORT=9100
DEMO=$(mktemp -d)
stop_emulator() { ss -ltnpH "sport = :$PORT" | grep -o 'pid=[0-9]*' | cut -d= -f2 | xargs -r kill; }
trap 'stop_emulator; kill $(jobs -p) 2>/dev/null; rm -rf "$DEMO"' EXIT

cargo build --release -q
BIN=target/release/printr
export PRINTR_DATA_DIR="$DEMO/data" PRINTR_CACHE_DIR="$DEMO/cache" PRINTR_GLITCH=0
# Mots fléchés : dictionnaire et définitions gardés d'une fois sur l'autre (pas de nouvel appel
# à Claude pour une grille déjà définie).
export CROSSWORDS_CACHE="${CROSSWORDS_CACHE:-$HOME/.cache/crosswords}"
mkdir -p "$PRINTR_DATA_DIR"

# Données fictives : une liste de courses et un mois d'impressions.
python3 -I - "$PRINTR_DATA_DIR/printr.json" "$PRINTR_LANG" <<'EOF'
import json, random, sys
from datetime import datetime, timedelta, timezone

rng = random.Random(42)
now = datetime.now(timezone.utc)
en = sys.argv[2] == "en"
shopping = [
    ("Lait", "Camille"), ("Pain de mie", "Léo"), ("Pommes", "Camille"), ("Café", "Alex"),
    ("Céréales au chocolat", "Léo"), ("Lessive", "Alex"), ("Tomates", "Camille"), ("Rouleaux de papier thermique", "Alex"),
]
if en:
    shopping = [
        ("Milk", "Camille"), ("Sandwich bread", "Léo"), ("Apples", "Camille"), ("Coffee", "Alex"),
        ("Chocolate cereal", "Léo"), ("Laundry detergent", "Alex"), ("Tomatoes", "Camille"), ("Thermal paper rolls", "Alex"),
    ]
people = ["Camille"] * 9 + ["Alex"] * 6 + ["Léo"] * 5 + ["planification"] * 7
tickets = [["date", "météo", "saint du jour", "citation"], ["sudoku"], ["énigme", "défi sportif"],
           ["horoscope Barnum"], ["image", "texte"], ["mots mêlés"], ["liste de courses"], ["sudoku", "énigme"]]
if en:
    tickets = [["date", "weather", "name day", "quote"], ["sudoku"], ["riddle", "workout"],
               ["Barnum horoscope"], ["image", "text"], ["word search"], ["shopping list"], ["sudoku", "riddle"]]
first = now.replace(day=1, hour=0, minute=0, second=0, microsecond=0)
history = []
for _ in range(40):
    at = first + timedelta(minutes=rng.randrange(max(1, int((now - first).total_seconds() // 60))))
    if rng.random() < 0.4:
        at = at.replace(hour=7, minute=30)
    history.append({
        "at": at.isoformat(), "by": rng.choice(people), "label": "Ticket", "ok": rng.random() > 0.05,
        "blocks": rng.choice(tickets), "paper_mm": rng.randrange(90, 320),
    })
history.sort(key=lambda e: e["at"])
data = {
    "shopping": [{"id": f"x{i}", "text": t, "by": b, "added": now.isoformat()} for i, (t, b) in enumerate(shopping)],
    "history": history,
}
json.dump(data, open(sys.argv[1], "w"), ensure_ascii=False, indent=1)
EOF

if nc -z 127.0.0.1 $PORT 2>/dev/null; then echo "le port $PORT est déjà utilisé" >&2; exit 1; fi
(cd emulator && exec uvx emupos run >"$DEMO/emupos.log" 2>&1) &
until nc -z 127.0.0.1 $PORT 2>/dev/null; do sleep 0.3; done
mkdir -p emulator/receipts "$OUT"

names=("$@")
if [ ${#names[@]} -eq 0 ]; then
  for f in docs/exemples/*.json; do
    name=$(basename "$f" .json)
    [ "$name" = catalogue ] || names+=("$name")
  done
fi
for name in "${names[@]}"; do
  find emulator/receipts -name '*.png' | sort >"$DEMO/before"
  before=$(wc -l <"$DEMO/before")
  source="docs/exemples/$name.json"
  [ -f "$OUT/$name.json" ] && source="$OUT/$name.json"
  $BIN -q --tcp "127.0.0.1:$PORT" print "$source"
  # Un ticket vide ne produit aucun rendu : on n'attend pas indéfiniment, on passe au suivant.
  waited=0
  until [ "$(find emulator/receipts -name '*.png' | wc -l)" -gt "$before" ]; do
    sleep 0.3
    waited=$((waited + 1))
    if [ "$waited" -ge 200 ]; then echo "$name : pas de rendu après 60 s, ignoré" >&2; continue 2; fi
  done
  # Un ticket avec un bloc coupe sort en plusieurs morceaux : on attend qu'il n'en arrive plus.
  count=0
  while [ "$(find emulator/receipts -name '*.png' | wc -l)" -ne "$count" ]; do
    count=$(find emulator/receipts -name '*.png' | wc -l)
    sleep 1
  done
  mapfile -t pieces < <(find emulator/receipts -name '*.png' | sort | comm -13 "$DEMO/before" -)
  if [ ${#pieces[@]} -eq 1 ]; then
    cp "${pieces[0]}" "$OUT/$name.png"
  else
    # Les morceaux l'un sous l'autre, séparés par un trait de coupe pointillé.
    uv run -q --with pillow python -I - "$OUT/$name.png" "${pieces[@]}" <<'EOF'
import sys
from PIL import Image, ImageDraw

pieces = [Image.open(p).convert("L") for p in sys.argv[2:]]
gap = 48
width = max(p.width for p in pieces)
out = Image.new("L", (width, sum(p.height for p in pieces) + gap * (len(pieces) - 1)), 255)
draw = ImageDraw.Draw(out)
y = 0
for i, piece in enumerate(pieces):
    if i:
        for x in range(0, width, 16):
            draw.line([(x, y - gap // 2), (x + 8, y - gap // 2)], fill=0, width=2)
    out.paste(piece, (0, y))
    y += piece.height + gap
out.save(sys.argv[1], optimize=True)
EOF
  fi
  chmod 644 "$OUT/$name.png"
  echo "$OUT/$name.png"
done
