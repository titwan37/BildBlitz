# BildBlitz super cool features

------------------------------

Pour propulser BildBlitz au rang d'application incontournable (must-have) face à des géants bien installés comme IrfanView, XnView MP ou Digikam, l'application doit exploiter ses deux forces majeures actuelles : son architecture Rust ultra-rapide (zéro latence) et son moteur de forces déterminantes accéléré par GPU (CUDA/cuBLAS).
Voici la feuille de route des fonctionnalités à haute valeur ajoutée à intégrer pour transformer BildBlitz en un outil de productivité révolutionnaire pour les photographes, designers et gestionnaires de données

## 1. 🔍 Recherche Sémantique Locale & "Zero-Shot" (via CLIP / SigLIP)

Au lieu de chercher par nom de fichier, l'utilisateur cherche par intention ou concept visuel.

* Principe : Utiliser la brique TensorEngine déjà prévue (via Candle ou ONNX Runtime) pour faire tourner localement un modèle de vision-langage ultra-léger (ex: MobileCLIP ou SigLIP).
* Fonctionnalités :
* Recherche en langage naturel : Taper "voiture de sport rouge sous la pluie" ou "coucher de soleil sur les montagnes suisses" trouve instantanément les images sans aucun tag manuel préalable.
  * Recherche par similarité visuelle (Reverse Image Search) : Cliquer sur un bouton "Trouver des images similaires" pour projeter l'image sélectionnée dans l'espace vectoriel et afficher les photos les plus proches (distance cosinus via cuBLAS).
  * Poids de la Force : Ajouter une 7ème force déterminante : la Force Sémantique.

## 2. 🗂️ Tri Intelligent Automatique & "Smart Folders" Dynamiques

Exploiter les 6 forces déterminantes actuelles pour organiser le chaos d'un disque dur en un clic.

* Principe : Permettre à l'utilisateur de configurer ses sliders de forces (Temps, Couleur, Croquis, Raytrace, etc.) et de cliquer sur "Générer l'arborescence".
* Fonctionnalités :
* Collections Virtuelles Prédictives : Création automatique de dossiers virtuels dans la base SQLite (ex: "Tous mes croquis du mois de mai", "Rendus 3D bruités à relancer").
  * Nettoyage de Rushes (Culling Engine) : Regroupement automatique des photos prises dans la même minute (Force Temporelle). BildBlitz compare les pHash et les histogrammes pour détecter les doublons ou les photos floues/ratées et propose de ne garder que la meilleure de la série.

## 3. 👥 Regroupement par Visages (Face Clustering) 100% Local & Privé

Le tri par visage de Google Photos ou Apple Photos, mais totalement hors-ligne, respectueux de la vie privée et ultra-rapide.

* Principe : Intégrer un pipeline de détection faciale léger (comme UltraFace ou InsightFace) s'exécutant dans le pool de threads Rayon ou sur le GPU.
* Fonctionnalités :
* Détection automatique des visages lors du scan de dossier.
  * Regroupement automatique par clusters d'identités (K-Means GPU).
  * L'utilisateur donne un nom à un groupe de visages, et BildBlitz met à jour l'index SQLite.

## 4. 🔀 Workflow de Renommage et Déplacement de Masse Batch "Zéro-Effort"

Remplacer les outils lourds comme Advance Renamer par une intégration fluide en double-panneau.

* Principe : Utiliser la puissance de calcul asynchrone (Tokio/Rayon) combinée à l'analyse de motifs de texte pour traiter des milliers de fichiers sans bloquer l'IHM.
* Fonctionnalités :
* Renommage Sémantique Intelligent : Détecter les structures communes (votre bug B7 sur le byte-slice étant corrigé) et proposer un renommage automatique basé sur la date EXIF, le score de style (ex: [Sketch]_Nom_001.jpg), ou le contenu sémantique.
  * Déplacement Transversal Sécurisé : Permettre de glisser-déposer des clusters entiers d'un panneau à l'autre avec vérification active anti-collision (évite d'écraser des fichiers existants, corrigeant le risque S2).

## 5. 🗜️ Pipeline de Conversion & Optimisation de Masse WebP / AVIF

Le couteau suisse absolu pour les créateurs de contenu Web.

* Principe : Utiliser le multi-threading de Rayon pour compresser et convertir des dossiers entiers d'images (RAW, PNG, Jpeg) vers les formats modernes de diffusion.
* Fonctionnalités :
* Resize & Compress automatique au lâcher de fichiers (Drag & Drop) dans un dossier spécifique.
  * Nettoyage EXIF à la volée (Optionnel, pour protéger la vie privée avant publication sur le web).
  * Aperçu Avant/Après instantané en double panneau (image originale à gauche avec son poids, image compressée à droite avec son poids estimé et son niveau de dégradation PSNR).

------------------------------

## 🛠️ Matrice de Priorité d'Implémentation Technique

Pour garder BildBlitz stable et fidèle à son architecture "Immediate-Mode" légère, voici l'ordre recommandé d'intégration :

| Fonctionnalité | Difficulté d'intégration | Impact Utilisateur | Composants requis |
| --- | --- | --- | --- |
| 1. Tri Intelligent / Culling | 🟢 Faible | 🔴 Très Élevé | Algorithmes existants (Sobel, Otsu, pHash, SQLite) |
| 2. Batch Renamer & Move | 🟡 Moyenne | 🟡 Élevé | Correction complète des I/O bloquants (Phase 2 du plan) |
| 3. Recherche Sémantique (CLIP) | 🔴 Élevée | 🔥 Révolutionnaire | TensorEngine (Candle/CUDA/DirectML) |
| 4. Optimiseur WebP / AVIF | 🟡 Moyenne | 🟡 Élevé | Pool Rayon + Encodeurs image / ravif |
| 5. Face Clustering | 🔴 Élevée | 🟡 Élevé | Modèle ONNX local + ONNX Runtime |

Pour continuer le développement, préférez-vous que nous commencions par implémenter le moteur de tri intelligent (Culling / Smart Folders) en utilisant les forces de rendu que nous venons de coder, ou préférez-vous d'abord sécuriser et corriger les 14 bugs critiques et bloquants identifiés dans la revue de code ?
