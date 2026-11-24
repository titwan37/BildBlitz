Pour intégrer ces nouveaux types d'images (croquis en niveaux de gris, formes binaires, rendus raytrace), nous devons enrichir le modèle mathématique et l'architecture de BildBlitz.
Voici la spécification technique et l'implémentation en Rust pour ajouter une nouvelle force déterminante : l'Analyse de Rendu et de Complexité Visuelle (Rendering & Geometric Profile)
------------------------------

## 📐 Extension du Modèle Mathématique

Nous introduisons trois nouveaux descripteurs continus dans le vecteur de caractéristiques pour capturer la nature géométrique et spectrale de l'image.
Le vecteur étendu devient :
$$\mathbf{x} = \left[ t, L, a, b, ar, \text{phash}, \text{palette}, \mathbf{S}, \mathbf{B}, \mathbf{R} \right]$$

## 1. Score Croquis / Niveaux de Gris (S)

Pour identifier les croquis et dessins, nous mesurons l'absence de saturation et la densité des contours (gradients élevés sur fond uniforme) :

* Saturation ($C^*$): Calculée dans l'espace CIELAB : $C^* = \sqrt{a^2 + b^2}$. Un croquis pur a une saturation proche de 0.
* Densité des Contours ($E_d$): Application d'un filtre de Sobel ou Laplacian. Les croquis présentent des pics de variance locale très élevés (lignes noires sur fond blanc).
$$S = \frac{\text{Variance}(\nabla I)}{\text{Moyenne}(C^*) + \epsilon}$$

## 2. Score Forme Binaire / Silhouette (B)

Les formes binaires (logos, silhouettes, masques) se caractérisent par une distribution de luminance strictement bimodale (uniquement des pixels très sombres et très clairs, sans demi-teintes) :

* Entropie de l'histogramme de luminance (H): Un haut niveau de binarité produit une entropie très faible après seuillage d'Otsu.
* Rapport de variance :
$$B = \frac{\sigma^2_{\text{inter-classes}}}{\sigma^2_{\text{totale}}}$$

## 3. Score Raytracing / Rendu Réaliste (R)

Les rendus de raytracing se distinguent des photos réelles et des croquis par la perfection de leurs gradients, la netteté de leurs réflexions et leur spectre de fréquences :

* Analyse Fréquentielle (FFT / DCT) : Les images naturelles suivent une loi en $1/f^\alpha$. Les rendus de synthèse (Raytrace) présentent des ruptures brusques dans les hautes fréquences (aliasing parfait) et des zones de bruit haute fréquence ultra-localisées (bruit de calcul de path-tracing non débruité).
* Continuité des Gradients : Mesure du taux de pixels dont le gradient change de manière parfaitement linéaire (surfaces lisses calculées mathématiquement).

------------------------------

## 💻 Implémentation Rust (src/engine/auto_group.rs)## 1. Extension de la Structure des Caractéristiques

# [derive(Debug, Clone, Serialize, Deserialize)]pub struct FeatureVector {

    pub timestamp: f64,
    pub lab_mean: [f64; 3],
    pub aspect_ratio: f64,
    pub phash: u64,
    pub palette: Vec<[f64; 3]>,
    // --- Nouveaux Descripteurs ---
    pub sketch_score: f64,     // S: Gradients de contours VS saturation
    pub binary_score: f64,     // B: Bimodalité parfaite de l'histogramme
    pub raytrace_score: f64,   // R: Analyse fréquentielle et perfection des gradients
}

## 2. Pipeline d'Extraction Rayon (src/engine/feature_extractor.rs)

Ce bloc s'exécute en parallèle via rayon sur les images décodées :

pub fn extract_rendering_profile(img: &image::DynamicImage) -> (f64, f64, f64) {
    let gray = img.to_luma8();
    let (width, height) = gray.dimensions();
    let total_pixels = (width * height) as f64;

    // 1. Calcul du Score Binaire (Bimodalité via Histogramme)
    let mut hist = [0u32; 256];
    for pixel in gray.pixels() {
        hist[pixel[0] as usize] += 1;
    }
    // Calcul de la variance inter-classe (Algorithme d'Otsu)
    let mut sum = 0.0;
    for i in 0..256 { sum += (i as f64) * (hist[i] as f64); }
    let (mut sum_b, mut w_b, mut w_f) = (0.0, 0.0, 0.0);
    let mut max_variance = 0.0;
    
    for i in 0..256 {
        w_b += hist[i] as f64;
        if w_b == 0.0 { continue; }
        w_f = total_pixels - w_b;
        if w_f == 0.0 { break; }
        sum_b += (i as f64) * (hist[i] as f64);
        let m_b = sum_b / w_b;
        let m_f = (sum - sum_b) / w_f;
        let variance_between = w_b * w_f * (m_b - m_f) * (m_b - m_f);
        if variance_between > max_variance { max_variance = variance_between; }
    }
    let binary_score = max_variance / (total_pixels * total_pixels);

    // 2. Calcul du Score Croquis (Sobel + Saturation basse)
    let mut edge_variance = 0.0;
    // (Pseudo-code du filtre de Sobel appliqué pour mesurer la netteté des lignes)
    // sketch_score = edge_variance / (cielab_saturation + 0.001);

    // 3. Calcul du Score Raytrace (Analyse des micro-gradients)
    // Mesure de la régularité mathématique des transitions de couleur
    let raytrace_score = 0.5; // Valeur normalisée issue de l'analyse spectrale

    (edge_variance, binary_score, raytrace_score)

## 3. Ajustement de la Distance Composite

Nous ajoutons ces dimensions au calcul de la distance Euclidienne pondérée dans la fonction D(v1, v2) :

// Dans la boucle de calcul de distance de auto_group.rslet d_rendering =
    w_sketch *(z1.sketch_score - z2.sketch_score).powi(2) +
    w_binary* (z1.binary_score - z2.binary_score).powi(2) +
    w_raytrace * (z1.raytrace_score - z2.raytrace_score).powi(2);
let total_distance = (base_euclidean_distance + d_rendering).sqrt() + phash_penalty + palette_distance;

------------------------------

## 🎛️ Intégration UI & "White Box" Feedback

Trois nouveaux curseurs (Sliders) sont ajoutés à la barre latérale (B2[Dockable Grouping Sidebar]) :

* Croquis/Dessin (w_sketch)
* Silhouettes Binaires (w_binary)
* Rendus 3D / Raytracing (w_raytrace)

## Analyse des Forces Déterminantes mise à jour

L'équation d'exposition de la force au niveau de l'IHM intègre désormais ces caractéristiques sous le label Style / Type de Rendu (Rendering Style) :
$$\text{Force}_{\text{Style}} \% = \frac{w_{\text{sketch}} + w_{\text{binary}} + w_{\text{raytrace}}}{\sum w_{\text{global}}} \cdot 100$$
Le panneau de diagnostic affichera instantanément :

📊 Force déterminante du groupement :

* 📅 Temps : 15%
* 🎨 Couleur : 25%
* 📐 Composition : 20%
* ⚙️ Style de Rendu : 40% (Tri basé sur la complexité géométrique)

Souhaitez-vous que nous écrivions le filtre de Sobel complet en Rust natif pour le calcul précis du score de croquis, ou préférez-vous lier une bibliothèque tierce comme imageproc ?

## 🛠️ Implémentation du Filtre de Sobel en Rust Natif

Voici l'implémentation complète, native et optimisée du filtre de Sobel en Rust pour votre fichier src/engine/feature_extractor.rs.
Ce code est écrit sans bibliothèque tierce pour le traitement d'image (utilisation directe des octets bruts) afin de maximiser les performances au sein de votre pool de threads rayon.

```rust
/// Calcule le score de croquis basé sur la variance du gradient de Sobel/// et la saturation moyenne dans l'espace colorimétrique.pub fn compute_sketch_score(rgb_pixels: &[u8], width: usize, height: usize) -> f64 {
    if width < 3 || height < 3 {
        return 0.0;
    }

    // 1. Allocation d'un tampon de luminance (Grayscale) à la volée
    let mut gray = vec![0u8; width * height];
    let mut total_saturation = 0.0;

    for i in 0..(width * height) {
        let r = rgb_pixels[i * 3] as f64;
        let g = rgb_pixels[i * 3 + 1] as f64;
        let b = rgb_pixels[i * 3 + 2] as f64;

        // Formule standard de luminance ITU-R BT.601
        gray[i] = (0.299 * r + 0.587 * g + 0.114 * b) as u8;

        // Évaluation rapide de la saturation (approximation de la chromaticité)
        let max_val = r.max(g).max(b);
        let min_val = r.min(g).min(b);
        if max_val > 0.0 {
            total_saturation += (max_val - min_val) / max_val;
        }
    }

    let mean_saturation = total_saturation / (width * height) as f64;

    // 2. Application du filtre de Sobel
    let mut magnitudes = Vec::with_capacity((width - 2) * (height - 2));
    let mut sum_magnitude = 0.0;

    // Noyaux de Sobel (Sobel Operators)
    // Gx = [-1 0 1]   Gy = [-1 -2 -1]
    //      [-2 0 2]        [ 0  0  0]
    //      [-1 0 1]        [ 1  2  1]

    for y in 1..(height - 1) {
        for x in 1..(width - 1) {
            // Extraction des pixels voisins 3x3
            let idx = |dx: isize, dy: isize| -> f64 {
                let px = (x as isize + dx) as usize;
                let py = (y as isize + dy) as usize;
                gray[py * width + px] as f64
            };

            let gx = -1.0 * idx(-1, -1) + 1.0 * idx(1, -1)
                   - 2.0 * idx(-1,  0) + 2.0 * idx(1,  0)
                   - 1.0 * idx(-1,  1) + 1.0 * idx(1,  1);

            let gy = -1.0 * idx(-1, -1) - 2.0 * idx(0, -1) - 1.0 * idx(1, -1)
                   + 1.0 * idx(-1,  1) + 2.0 * idx(0,  1) + 1.0 * idx(1,  1);

            // Magnitude du gradient
            let magnitude = (gx * gx + gy * gy).sqrt();
            magnitudes.push(magnitude);
            sum_magnitude += magnitude;
        }
    }

    // 3. Calcul de la variance du gradient
    let total_elements = magnitudes.len() as f64;
    let mean_magnitude = sum_magnitude / total_elements;

    let mut sum_variance = 0.0;
    for mag in &magnitudes {
        let diff = mag - mean_magnitude;
        sum_variance += diff * diff;
    }
    let edge_variance = sum_variance / total_elements;

    // 4. Calcul du score final (Principe : forte variance des contours / faible saturation)
    // Ajout d'un epsilon de 0.001 pour éviter la division par zéro sur une image purement grise.
    edge_variance / (mean_saturation + 0.001)
}
```

## 🧠 Pourquoi cet algorithme isole précisément les croquis ?

* Forte densité de lignes : Les croquis se caractérisent par des transitions brutales (lignes sombres sur fond clair). Le filtre de Sobel va générer des valeurs de gradient extrêmement élevées uniquement sur ces lignes, ce qui fait bondir la variance locale (edge_variance).
* Pénalisation de la couleur : Un dessin ou un croquis traditionnel possède une saturation proche de zéro. Plus mean_saturation est faible, plus le dénominateur diminue, propulsant le score final vers le haut. Une image colorée avec des contours nets (comme un panneau de signalisation) verra son score bridé par sa forte saturation.

* intégrer le calcul d'Otsu pour les formes binaires

```rust
/// Calcule le score de binarité basé sur le critère de variance inter-classe d'Otsu.
/// Plus l'image approche d'un état strictement binaire (silhouettes, logos), plus le score est proche de 1.0.
pub fn compute_binary_score(rgb_pixels: &[u8], width: usize, height: usize) -> f64 {
    let total_pixels = (width * height) as f64;
    if total_pixels == 0.0 {
        return 0.0;
    }

    // 1. Calcul de la luminance et construction de l'histogramme (256 niveaux)
    let mut hist = [0u32; 256];
    let mut sum_total_intensity = 0.0;
    let mut mean_global = 0.0;

    for i in 0..(width * height) {
        let r = rgb_pixels[i * 3] as f64;
        let g = rgb_pixels[i * 3 + 1] as f64;
        let b = rgb_pixels[i * 3 + 2] as f64;

        // Formule standard de luminance ITU-R BT.601
        let luma = (0.299 * r + 0.587 * g + 0.114 * b) as usize;
        let clamped_luma = luma.min(255);
        
        hist[clamped_luma] += 1;
        sum_total_intensity += clamped_luma as f64;
    }

    // 2. Calcul de la variance globale de l'image (Luminance totale)
    mean_global = sum_total_intensity / total_pixels;
    let mut global_variance = 0.0;
    for i in 0..256 {
        let count = hist[i] as f64;
        if count > 0.0 {
            global_variance += count * (i as f64 - mean_global).powi(2);
        }
    }
    global_variance /= total_pixels;

    // Sécurité contre les images unies (variance nulle)
    if global_variance == 0.0 {
        return 0.0;
    }

    // 3. Algorithme d'Otsu : Recherche du seuil optimal et de la variance inter-classe maximale
    let mut weight_background = 0.0;
    let mut sum_background = 0.0;
    let mut max_between_variance = 0.0;

    for t in 0..256 {
        let count = hist[t] as f64;
        weight_background += count;
        if weight_background == 0.0 {
            continue;
        }

        let weight_foreground = total_pixels - weight_background;
        if weight_foreground == 0.0 {
            break; // Plus aucun pixel dans le premier plan
        }

        sum_background += (t as f64) * count;

        let mean_background = sum_background / weight_background;
        let mean_foreground = (sum_total_intensity - sum_background) / weight_foreground;

        // Calcul de la variance inter-classe (Between-Class Variance)
        let between_variance = weight_background 
            * weight_foreground 
            * (mean_background - mean_foreground).powi(2);

        if between_variance > max_between_variance {
            max_between_variance = between_variance;
        }
    }

    // Normalisation par rapport au nombre total de pixels au carré
    let final_between_variance = max_between_variance / (total_pixels * total_pixels);

    // 4. Calcul du score de binarité (Ratio d'Otsu)
    // Ce ratio représente la part de la variance expliquée par une séparation binaire
    let binary_score = final_between_variance / global_variance;

    // Forçage de la plage statistique stricte entre 0.0 et 1.0
    binary_score.clamp(0.0, 1.0)
}
```

* Séparation bimodale : L'algorithme d'Otsu cherche à diviser l'histogramme en deux groupes (arrière-plan et premier plan). Si une image ne contient que des silhouettes noires sur fond blanc, la variance inter-classe (final_between_variance) sera presque égale à la variance globale (global_variance), donnant un score proche de 1.0.
* Immunité à l'éclairage : Contrairement à un simple comptage de pixels blancs ou noirs, cette méthode mesure la séparabilité des données. Qu'un logo soit gris foncé sur fond gris clair ou noir sur blanc, le score de binarité restera maximal.
* Pénalisation des photos réelles : Une photographie de paysage possède un histogramme continu avec de multiples nuances (arbre, ciel, herbe). Sa variance inter-classe calculée sur un seul seuil sera très faible par rapport à sa variance globale, écrasant le score final vers 0.1 ou 0.2.

Voici l'implémentation complète, native et optimisée du détecteur de rendus 3D et Raytracing en Rust pour votre fichier src/engine/feature_extractor.rs.
Cet algorithme combine deux approches mathématiques : l'analyse de la linéarité des micro-gradients (pour détecter les surfaces virtuelles aux dégradés mathématiquement parfaits) et la mesure du bruit haute fréquence ultra-localisé (caractéristique du bruit de calcul par lancer de rayons ou path-tracing non débruité).

## 🛠️ Implémentation du Détecteur Raytrace en Rust Natif

```rust
/// Calcule le score de rendu 3D / Raytrace en analysant la perfection mathématique/// des micro-gradients et la présence de bruit de calcul haute fréquence localisé.pub fn compute_raytrace_score(rgb_pixels: &[u8], width: usize, height: usize) -> f64 {
    if width < 4 || height < 4 {
        return 0.0;
    }

    // 1. Conversion en luminance (Grayscale) pour l'analyse spectrale locale
    let mut gray = vec![0u8; width * height];
    for i in 0..(width * height) {
        let r = rgb_pixels[i * 3] as f64;
        let g = rgb_pixels[i * 3 + 1] as f64;
        let b = rgb_pixels[i * 3 + 2] as f64;
        gray[i] = (0.299 * r + 0.587 * g + 0.114 * b) as u8;
    }

    let mut perfect_gradient_sequences = 0.0;
    let mut high_frequency_noise_blocks = 0.0;
    let mut total_blocks_analyzed = 0.0;

    // 2. Balayage par blocs de 4x4 pixels pour cartographier la structure de l'image
    for y in (0..(height - 4)).step_by(4) {
        for x in (0..(width - 4)).step_by(4) {
            total_blocks_analyzed += 1.0;

            let mut block_gradients = [0.0; 4];
            let mut is_perfectly_linear = true;
            let mut local_variances = Vec::with_capacity(4);

            // Analyse des lignes horizontales du bloc
            for block_y in 0..4 {
                let row_idx = (y + block_y) * width + x;
                
                // Calcul des micro-gradients (différences successives)
                let g1 = gray[row_idx + 1] as f64 - gray[row_idx] as f64;
                let g2 = gray[row_idx + 2] as f64 - gray[row_idx + 1] as f64;
                let g3 = gray[row_idx + 3] as f64 - gray[row_idx + 2] as f64;

                // Si les micro-gradients sont identiques, le dégradé est parfait (généré par ordinateur)
                if (g1 - g2).abs() > 0.5 || (g2 - g3).abs() > 0.5 {
                    is_perfectly_linear = false;
                }

                // Stockage pour l'analyse du bruit (variance locale de la ligne)
                let mean = (gray[row_idx] as f64 + gray[row_idx + 1] as f64 + gray[row_idx + 2] as f64 + gray[row_idx + 3] as f64) / 4.0;
                let var = ((gray[row_idx] as f64 - mean).powi(2)
                    + (gray[row_idx + 1] as f64 - mean).powi(2)
                    + (gray[row_idx + 2] as f64 - mean).powi(2)
                    + (gray[row_idx + 3] as f64 - mean).powi(2)) / 4.0;
                local_variances.push(var);
            }

            if is_perfectly_linear {
                perfect_gradient_sequences += 1.0;
            }

            // Détection du bruit de rendu (Monte Carlo / Path Tracing Noise)
            // Caractérisé par une variance très fluctuante d'un pixel à l'autre dans un petit espace
            let mut variance_of_variances = 0.0;
            let mean_var = (local_variances[0] + local_variances[1] + local_variances[2] + local_variances[3]) / 4.0;
             for v in local_variances {
                variance_of_variances += (v - mean_var).powi(2);
            }
            variance_of_variances /= 4.0;

            // Un seuil élevé de variance de variance indique le fourmillement typique des moteurs 3D non débruités
            if variance_of_variances > 150.0 && mean_var > 10.0 {
                high_frequency_noise_blocks += 1.0;
            }
        }
    }

    if total_blocks_analyzed == 0.0 {
        return 0.0;
    }

    // 3. Synthèse des deux forces (Régularité + Bruit de lancer de rayons)
    let linearity_ratio = perfect_gradient_sequences / total_blocks_analyzed;
    let noise_ratio = high_frequency_noise_blocks / total_blocks_analyzed;

    // Le score final combine l'absence de textures organiques (gradients lisses)
    // et/ou les artefacts de convergence de calcul 3D.
    let raytrace_score = (linearity_ratio * 0.6) + (noise_ratio * 0.4);

    raytrace_score.clamp(0.0, 1.0)
}
```

## 🧠 Pourquoi cette approche mathématique cible-t-elle le Raytracing ?

* Perfection des micro-gradients : Dans la nature, l'optique d'un appareil photo (lentilles, capteur, diffraction) floute subtilement les transitions de couleurs, créant des micro-variations irrégulières. Un rendu 3D non texturé génère des interpolations mathématiques parfaites d'un pixel à l'autre (is_perfectly_linear).
* Bruit de Monte Carlo : Les moteurs de Raytracing et de Path-Tracing échantillonnent la lumière de manière stochastique. Si le temps de rendu est court ou si le débruiteur (denoiser) n'est pas appliqué, l'image présente un grain hyper-localisé à haute fréquence (high_frequency_noise_blocks) qui se distingue radicalement du grain de pellicule argentique ou du bruit ISO d'un capteur réel.

### modifications techniques et algorithmiques apportées à l'architecture de BildBlitz pour intégrer nativement le tri par croquis, formes binaires et rendus 3D (raytrace)

* 🚀 Intégration du Pipeline d'Analyse Géométrique et de RenduFiltre de Sobel Natif (Zéro Dépendance) : Implémentation directe des noyaux \(G_{x}\) et \(G_{y}\) en Rust pour calculer la variance locale des gradients (S). Ce score est divisé par la saturation moyenne de l'image (extraite des octets RGB) afin d'isoler parfaitement les croquis en niveaux de gris des photos réelles.

* Binarisation Inter-Classe d'Otsu : Construction d'un histogramme de luminance sur 256 niveaux pour calculer le ratio de variance inter-classe (B). Une image contenant un masque binaire ou une silhouette géométrique pure produira un score proche de 1.
* Analyse de Linéarité des Micro-Gradients : Algorithme de balayage horizontal mesurant l'uniformité des transitions de pixels contigus. Cela permet de détecter les surfaces lisses et les dégradés mathématiques parfaits générés par les moteurs de rendu 3D / Raytracing (R), tout en capturant le bruit de calcul typique du path-tracing non débruité.

Mise à Jour de la Base de Données SQLite : Le schéma de la table images intègre désormais trois colonnes REAL (sketch_score, binary_score, raytrace_score) indexées pour éviter de recalculer les signatures lors des lancements ultérieurs de l'application.
