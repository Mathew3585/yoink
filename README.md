<h1 align="center">🎣 Yoink</h1>

<p align="center">
  Un téléchargeur de vidéos rapide et élégant pour Windows.<br>
  Vidéo <strong>MP4</strong> (jusqu'à la 4K/8K) ou audio <strong>MP3</strong>, en deux clics.
</p>

<p align="center">
  <img alt="Tauri" src="https://img.shields.io/badge/Tauri-2-FFC131?logo=tauri&logoColor=white">
  <img alt="Rust" src="https://img.shields.io/badge/Rust-000?logo=rust&logoColor=white">
  <img alt="Plateforme" src="https://img.shields.io/badge/Windows-10%2F11-0078D6?logo=windows&logoColor=white">
</p>

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
