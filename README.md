# MessesInfo Scraper

CLI Rust qui interroge l'API `messes.info`, normalise les celebrations et produit:

- soit un JSON sur la sortie standard,
- soit un enregistrement dans une base SQLite.

## Fonctionnalites

- Recuperation des celebrations d'une paroisse (`--paroisse`)
- Conversion des heures (`19h15` -> `19:15`)
- Conversion de `length` en minutes (`1h00` -> `60`, `45min` -> `45`)
- Format date FR (`date_fr`)
- Ecriture SQLite optionnelle
- Option de purge de table avant insertion (`--clear-db`)

## Prerequis

Sous Debian/Ubuntu:

```bash
sudo apt update
sudo apt install curl build-essential pkg-config libssl-dev
```

Installation de Rust:

```bash
curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh
source "$HOME/.cargo/env"
```

Verification:

```bash
rustc --version
cargo --version
```

## Build

```bash
cargo build --release
```

## Aide CLI

```bash
cargo run -- -h
```

Options principales:

- `-p, --paroisse <URL>`: URL complete de la communaute (obligatoire)
- `-d, --database <SQLITE_PATH>`: chemin du fichier SQLite
- `--clear-db`: vide la table `messeinfo` avant insertion
- `-v, --verbose`: logs verbeux
- `-h, --help`: aide
- `-V, --version`: version

## Utilisation

1. Sortie JSON (sans base):

```bash
cargo run -- --paroisse https://messes.info/communaute/pa/75/filles-de-la-charite
```

2. Ecriture en base SQLite (sans sortie JSON):

```bash
cargo run -- --paroisse https://messes.info/communaute/pa/75/filles-de-la-charite --database a.db
```

3. Ecriture en base avec purge prealable:

```bash
cargo run -- --paroisse https://messes.info/communaute/pa/75/filles-de-la-charite --database a.db --clear-db
```

## Schema SQLite

Table creee automatiquement: `messeinfo`

- `id` (INTEGER, PK autoincrement)
- `date` (TEXT)
- `time` (TEXT)
- `locality` (TEXT)
- `length` (INTEGER, en minutes)
- `comment` (TEXT, nullable)
- `name` (TEXT, nullable)
- `type` (TEXT, nullable)
- `date_fr` (TEXT)

## Notes

- Si `--database` est fourni, la sortie JSON est desactivee.
- Le champ `length` de l'API peut arriver en texte (`1h00`, `30min`, `45min`): il est converti en minutes.
- Certaines entrees de la reponse API sont des objets de localite et sont ignorees par le parseur (seules les celebrations sont conservees).