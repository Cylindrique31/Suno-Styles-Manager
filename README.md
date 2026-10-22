# Gestionnaire de Styles Suno

Application de bureau écrite en Rust, avec une interface native `egui` et une base SQLite locale.

## Fonctionnalités
- Ajouter, modifier et supprimer des styles Suno
- Champs : Nom du style, Tags, Style, Exclusion
- Recherche instantanée avec cases à cocher pour choisir les champs ciblés : Nom du style, Tags, Style et Exclusion
- Boutons de copie pour les champs Style et Exclusion
- Favoris : étoile cliquable à la fin de chaque ligne et onglet « Favoris »
- Confirmation avant suppression
- Enregistrement persistant dans SQLite
- Export de tous les styles vers un fichier JSON
- Import depuis un fichier JSON, en ajoutant les styles dont le nom n’existe pas déjà (comparaison insensible à la casse)

## Prérequis
- Rust et Cargo (installation via https://rustup.rs)
- Sous Linux, les dépendances système nécessaires à `eframe`/`winit` et au presse-papiers peuvent être requises (notamment les bibliothèques X11/Wayland).

## Lancer l'application
Dans le dossier du projet :

```bash
cargo run --release
```

Au premier lancement, Cargo télécharge et compile les dépendances. La base est créée automatiquement dans le dossier de données utilisateur :
- Windows : `%LOCALAPPDATA%\\suno-styles\\suno_styles.sqlite3`
- Linux : `$XDG_DATA_HOME/suno-styles/suno_styles.sqlite3`, ou `~/.local/share/suno-styles/suno_styles.sqlite3`
- Si aucune variable de dossier de données n'est disponible, le dossier courant est utilisé.

## Compiler un exécutable

```bash
cargo build --release
```

L'exécutable se trouve ensuite dans `target/release/` (`suno-styles.exe` sous Windows).

## Import / Export

Les boutons **Importer** et **Exporter** de la barre d’outils utilisent une boîte de dialogue de fichiers. L’export crée un fichier JSON portable contenant les champs Nom, Tags, Style, Exclusion et l’état favori. Les bases SQLite des versions précédentes sont mises à niveau automatiquement, sans supprimer les styles existants. L’import ajoute les styles dont le nom n’existe pas déjà dans la bibliothèque ; les doublons (sans distinction majuscules/minuscules) et les entrées sans nom sont ignorés. L’import ne supprime et ne remplace aucun style existant.
