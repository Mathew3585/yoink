<div align="center">

<img src="assets/banner.svg" alt="Yoink" width="100%">

<br>

**Colle un lien. Choisis le format. Récupère ta vidéo — MP4 ou MP3, en deux clics.**

Yoink est un téléchargeur de vidéos pour Windows : rapide, sans pub, sans compte, open source.

[![License: MIT](https://img.shields.io/badge/License-MIT-ff3b30?style=flat-square)](LICENSE)
[![Plateforme](https://img.shields.io/badge/Windows-10%2F11-ff3b30?style=flat-square)](#-prérequis-développement)
[![Tauri](https://img.shields.io/badge/Tauri-2-ff3b30?style=flat-square)](https://tauri.app)
[![Rust](https://img.shields.io/badge/Rust-000?style=flat-square&logo=rust)](https://www.rust-lang.org)

</div>

---

## 🖼️ Aperçu

<div align="center">

<img src="assets/screen-download-dark.svg" alt="Vue téléchargement en mode sombre : aperçu de la vidéo, choix du format et de la qualité, barre de progression" width="88%">

<br><br>

<img src="assets/screen-history-light.svg" alt="Historique des téléchargements en mode clair : liste des vidéos récupérées avec miniature, format, qualité et date" width="88%">

</div>

---

## ✨ Fonctionnalités

- 🎬 **Vidéo (MP4 / MKV / WEBM / MOV)** — de la HD à la 4K/8K selon la source
- 🎵 **Audio (MP3 / M4A / OPUS / FLAC / WAV)** avec choix de la qualité
- 🌗 **Thèmes clair & sombre** (palette rouge)
- 🕑 **Historique** des téléchargements, avec ouverture directe du fichier
- ⚡ **Téléchargement accéléré** (fragments en parallèle) pour contourner le bridage
- 🎛️ **Ré-encodage GPU NVENC** (H.264 / HEVC) en option, pour les cartes NVIDIA
- 🔼 **Upscaling IA** optionnel (Real-ESRGAN)
- 📁 Choix du dossier de sortie (mémorisé)

Le téléchargement s'appuie sur [`yt-dlp`](https://github.com/yt-dlp/yt-dlp) + [`ffmpeg`](https://ffmpeg.org), embarqués dans l'application.

## 🛠️ Prérequis (développement)

- [Node.js](https://nodejs.org) 18+
- [Rust](https://rustup.rs)
- Outils de build Tauri (WebView2 est déjà présent sur Windows 10/11)

## 🚀 Installation

```powershell
npm install
# Télécharge yt-dlp.exe + ffmpeg.exe dans src-tauri/binaries
./scripts/setup-binaries.ps1
```

## 💻 Lancer en développement

```powershell
npm run tauri dev
```

## 📦 Générer l'installateur

```powershell
npm run tauri build
```

L'installateur (`.exe` NSIS) est produit dans `src-tauri/target/release/bundle/`.

## 🔄 Mettre à jour les moteurs

Les plateformes changent souvent ; si un téléchargement échoue, relance simplement :

```powershell
./scripts/setup-binaries.ps1
```

(supprime `src-tauri/binaries/ffmpeg.exe` avant si tu veux aussi remettre ffmpeg à jour.)

## 📝 Notes

- **Sans perte par défaut** : la vidéo est téléchargée puis simplement remixée (audio+vidéo), sans ré-encodage.
- **NVENC** : le ré-encodage GPU n'est utile que pour changer de codec (H.264/HEVC) ; il peut légèrement réduire la qualité.

## ⚖️ Avertissement

Yoink est un outil à **usage personnel**. Respecte les conditions d'utilisation des plateformes et le droit d'auteur : ne télécharge que du contenu que tu as le droit de récupérer.

## 📄 Licence

MIT
