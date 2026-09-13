---
target: settings
total_score: 20
p0_count: 0
p1_count: 4
timestamp: 2026-09-12T14-09-02Z
slug: src-components-settingswindow-tsx
---
# Critique — fenêtre Paramètres (`src/components/SettingsWindow.tsx`)

## Design Health Score

| # | Heuristique | Score | Problème clé |
|---|---|---|---|
| 1 | Visibilité de l'état du système | 2 | Barre latérale mise à jour au clic seulement (`:797`), pas au défilement ; « Paramètres enregistrés ! » posé puis fenêtre cachée dans le même tick (`:546-548`) ; rechargement multi-Go du moteur après Enregistrer invisible |
| 2 | Correspondance avec le monde réel | 2 | « Déchargement », « Ollama », « Moteur STT (auto-hébergé) », « Direct (wtype/xdotool) » ; identifiants internes (« ggml-large-v3 ») ; unités anglaises (« 3100 MB », « 8.0 GB ») ; « Press any key... » (`:422`), « Error: » (`:550`) |
| 3 | Contrôle et liberté | 1 | Pas d'Annuler ; la croix cache une fenêtre à l'état modifié (`:679-681`) → réouverture qui ment ; blur pendant la capture → « Ctrl+Space » (`:490-494`) ; téléchargement de 3,1 Go sans annulation alors que `cancelModelDownload` existe ; pas de suppression de modèle |
| 4 | Cohérence et standards | 2 | Autostart / permissions / extension GNOME s'appliquent immédiatement, tout le reste attend Enregistrer ; badges tantôt indigo, tantôt `bg-green-600` forcé (`:1124`, `:1590`) ; focus 3 px partout sauf 1 px sur le curseur (`slider.tsx:17`) |
| 5 | Prévention des erreurs | 1 | **Enregistrer réécrit des réglages que l'écran n'affiche pas** : micro choisi par index de liste (`:177`, `:521` → mauvais micro si l'ordre change, observé sur cette machine : « HD-Audio Generic » affiché pour un micro USB Logitech) ; `custom_words` / `filler` / `stutter` jamais lus (`:167-205`) mais renvoyés (`:535-538`) → réinitialisés ; « Télécharger » actif sur les modèles « bientôt » (`:1242`) ; la capture accepte Tab/Entrée/Échap comme raccourci |
| 6 | Reconnaissance plutôt que rappel | 3 | Tout est visible et décrit ; « (Non installé) » oblige à retenir quoi chercher deux cartes plus bas |
| 7 | Flexibilité et efficacité | 2 | Ni Ctrl+S, ni Entrée, ni Échap ; capture de raccourci à la souris seulement ; seuil hybride : 117 pas sur un curseur sans champ numérique (`:1016-1022`) |
| 8 | Esthétique et minimalisme | 2 | Dix teintes d'icônes (`:727-791` + titres de cartes), boîte cyan (`:1083`), jaune hors tokens (`:1101`, `:1137`, `:1424`) ; redondances : « ⭐ Recommandé » + badge « Recommandé » + badge « Installé » + bouton « Installé ✓ » ; jusqu'à 6 boutons primaires dans Modèles |
| 9 | Diagnostic et récupération | 2 | Bon : compteur d'échecs GPU + « Réessayer », commande `gnome-extensions enable` donnée. Mauvais : erreurs en dialogues natifs avec la chaîne brute du backend (`:365`, `:581`) ; échec silencieux du chargement des modèles (`:241`) ; « Ollama non joignable » sans piste |
| 10 | Aide et documentation | 2 | Les phrases de « pourquoi » sont là ; aucun chemin des modèles / de la config alors que « où sont ses données » est un critère de succès (`getAppDataInfo` existe, inutilisé) |
| **Total** | | **20/40** | **Acceptable — bas de fourchette, pour la dette d'interaction et d'accessibilité, pas pour le visuel** |

## Anti-Patterns Verdict

**Évaluation LLM :** on ne dit pas « une IA a fait ça » — pas de dégradé, pas de verre, pas d'eyebrow, rayon 12 px, `shadow-sm`, corps de texte à 6,9:1. On dit « c'est un template shadcn sombre » : hors le texte, rien n'appartient à KaraokeTyper. Le tell le plus bruyant est l'anti-référence n° 3 de PRODUCT.md : dix teintes d'icônes différentes, plus l'icône `Sparkles` fuchsia pour Ollama (le badge ✨ interdit par DESIGN.md, `:1544`), des emoji dans la copy (⭐ `:939`, ⚠️ `:979`, ✓/○ `:1244`), deux options « bientôt » inertes dans un select (`:941-946`) et des tirets cadratins comme séparateurs d'options (`:937`, `:1321`).

**Scan déterministe :** CLI (`detect.mjs` sur `SettingsWindow.tsx`, `ui/`, `settings.tsx`, `settings.html`) : **0 constat** (détecteur validé par un fichier piège). En page (Chrome, Vite, `detect.js` injecté) : **21 anti-patterns** — `nested-cards` ×10 (les lignes de modèles et boîtes GPU/permissions sont des cartes dans des cartes : réel ; 9 occurrences sur le `Card` racine relèvent du wrapper), `low-contrast` ×2 (**#fafafa sur #6467f2 = 4,2:1**, boutons primaires et badges : réel, confirmé indépendamment par la revue LLM), `line-length` ×5 (mesuré à 1920 px de large ; à 820 px les paragraphes font ~70-80 caractères : artefact de viewport), `cramped-padding` ×2 (barre de titre `h-10 px-3` et bouton `sm` sans padding vertical : mineur), `clipped-overflow-container` ×2 (rail du `Progress` : faux positif structurel ; `h-screen overflow-hidden` : voulu), `single-font` / `overused-font` (pile système résolue en Roboto : voulu), `flat-type-hierarchy` (12/14/16/18 px : le 16 px du champ de saisie s'écarte de la Three Sizes Rule).

**Overlays visuels :** l'injection a réussi et une capture avec les 42 nœuds d'overlay est enregistrée dans le scratchpad (`settings-impeccable-overlays.jpg`) ; l'onglet a été fermé après la lecture de la console, il n'y a donc pas d'onglet [Human] ouvert en ce moment.

## Overall Impression

Le fond est juste : trois paliers de gris, bord 1 px, police système, trois tailles tenues, des phrases de « pourquoi » en bon français. Ce qui coince n'est pas le visuel mais la **confiance** : la fenêtre peut afficher un micro qui n'est pas celui du backend, réinitialiser des réglages invisibles à l'Enregistrer, perdre le raccourci sur un clic ailleurs, et disparaître sans dire qu'un chargement de 3 Go commence. La plus grande opportunité : faire d'Enregistrer une action fiable et visible, puis taire les dix couleurs.

## What's Working

1. **La copy de « pourquoi »** — « Parakeet V3 est bien plus rapide que Whisper : il transcrit les dictées courtes, Whisper reprend la main au-delà du seuil » (`:991`), « Libérer la mémoire… Le modèle se recharge automatiquement » (`:1513`). Une phrase, le pourquoi, puis silence : la One Sentence Rule appliquée.
2. **Les surfaces** — paliers #121212 / #1a1a1a / #242424, bord 1 px, `shadow-sm`, cartes 12 px, hiérarchie 18/14/12. Rien à démolir, seulement à taire.
3. **L'état montré, pas promis** — nom du GPU et VRAM, mode courant avec raison du repli, compteur d'échecs GPU avec « Réessayer », evdev/uinput cochés, « Tester » Ollama avec résultat ; divulgation progressive correcte (Ollama, volume, seuil hybride, carte GNOME conditionnelle).

## Priority Issues

**[P1] Enregistrer réécrit ce que l'écran ne montre pas**
- Quoi : micro sélectionné par *index* (`:177`, `:521`) — si l'ordre des périphériques change, Enregistrer bascule silencieusement sur un autre micro (observé : « HD-Audio Generic » affiché pour `plughw:CARD=U0x46d0x825`) ; `custom_words`, `custom_words_threshold`, `filler_removal_enabled`, `stutter_collapse_enabled` jamais chargés (`:167-205`) mais renvoyés avec les défauts du store (`:535-538`).
- Pourquoi : perte de données silencieuse au clic le plus fréquent de la fenêtre ; PRODUCT.md promet « l'utilisateur sait ce qui tourne ».
- Correctif : sélectionner le micro par `raw_name` (fallback index) ; round-tripper les 4 champs comme `keyDelayMsRef`, ou les retirer de `AppConfig` envoyé (envoyer un patch partiel).
- Commande : `$impeccable harden`

**[P1] Capture de raccourci : souris seule, valeur perdue au blur, copy anglaise** (`:419-495`, `:883-899`)
- Pourquoi : c'est *le* réglage du produit ; inaccessible au clavier (`onClick` sur un `readOnly`), `handleBlur` remplace la valeur par « Ctrl+Space », Tab/Entrée/Échap sont capturés comme raccourci, « Press any key... ».
- Correctif : champ + bouton « Modifier » (Entrée/Espace lancent la capture) ; mémoriser la valeur précédente et la restaurer sur blur/Échap ; ignorer Tab/Entrée/Échap ; « Appuyez sur une touche ou un bouton souris… Échap pour annuler ».
- Commande : `$impeccable harden`

**[P1] Fermer / Enregistrer : état fantôme, confirmation invisible, rechargement muet** (`:508-557`, `:679-681`)
- Pourquoi : la croix cache une fenêtre dont l'état React reste modifié → à la réouverture l'UI affiche des valeurs non enregistrées comme acquises ; pas d'Annuler ; « Paramètres enregistrés ! » n'est jamais vu ; si le modèle change, un chargement de plusieurs Go démarre en silence et la dictée suivante peut échouer sans avertissement.
- Correctif : `loadSettings()` à chaque `show` (ou reset du store à la fermeture) ; bouton « Annuler » fantôme à gauche du primaire ; après un save qui recharge, garder la fenêtre avec « Enregistré · chargement de Whisper Large-v3… » ou pousser l'état dans l'overlay ; Ctrl+S / Échap.
- Commande : `$impeccable harden`

**[P1] Accessibilité : aucune structure, contrôles sans nom, contraste** (`card.tsx:31-38`, `:838`, `:956`, `:1028`, `:1172`, `:1350`, `:1453`, `:1557`, `:1016`, `:695-718`, `:793-817`)
- Pourquoi : `CardTitle` rend un `div` → zéro `h2`, impossible de naviguer entre 10 sections au lecteur d'écran ; `Label htmlFor` pointe vers des `SelectTrigger` sans `id` ; 4 switches, 2 sliders, 3 boutons de barre de fenêtre sans nom accessible ; nav sans `aria-current` ni focus visible ; aucun `prefers-reduced-motion` ; anneau du curseur à 1 px ; **#fafafa sur #6467f2 = 4,2:1** (< 4,5:1 promis) sur tous les boutons primaires et badges — les deux évaluations convergent.
- Correctif : `<CardTitle asChild><h2>` ; `id` sur chaque trigger, `aria-label` sur sliders/icônes, `aria-current` sur l'entrée active, `focus-visible:ring-[3px]` sur nav et curseur ; `motion-reduce:animate-none` ; assombrir l'indigo d'un cran (≈ `hsl(239 72% 60%)`) pour passer 4,5:1 avec #fafafa.
- Commande : `$impeccable audit`

**[P2] One Signal Rule cassée, redondances de statut, six primaires par carte** (`:727-791`, titres de cartes, `:1083`, `:1124`, `:1212-1245`, `:1341-1355`)
- Pourquoi : anti-référence n° 3 et dette nommée par DESIGN.md ; `Sparkles` = badge IA ; un même bit encodé trois fois (badge + texte + switch ; badge + bouton « Installé ✓ ») ; six « Télécharger » indigo dans Modèles alors que DESIGN.md veut un seul primaire par vue.
- Correctif : icônes en `text-muted-foreground` ; `Sparkles` → `SpellCheck` ; boîte cyan → caption ; badges `default` uniquement, jamais `bg-green-600` ; « Télécharger » en `outline` ; supprimer le badge « Recommandé » codé en dur (`:1218`, le catalogue porte `is_recommended`) et les emoji.
- Commande : `$impeccable quieter`

**[P2] Architecture : 10 sections plates pour un écran « ponctuel et précis »** (`:726-818`, `:985-1048`, `:1251-1358`)
- Pourquoi : « Déchargement » est une section pour un select à 4 options ; « Ollama » est une marque ; « Système » agrège permissions + collage + autostart ; « Langue » suit le bloc hybride qu'elle devrait précéder ; « Modèle actif » et la liste des modèles sont à deux cartes de distance ; 1,5 à 2 cartes par écran (air d'en-tête 36 px au lieu des 12 px promis, `card.tsx:10` `gap-6` + `pb-3`) ; la barre ne suit pas le défilement.
- Correctif : 5 groupes — Entrée (micro, raccourci) / Reconnaissance (moteur, modèle actif + « Gérer » inline, langue, puis bloc Parakeet) / Machine (GPU, déchargement) / Sortie (collage, sons, correction, mots personnalisés) / Système & À propos ; `IntersectionObserver` pour `activeSection` ; `gap-6` → `gap-3`.
- Commande : `$impeccable layout`

## Persona Red Flags

**Alex (power user impatient)** — Veut passer le seuil de 20 à 12 s : curseur seul, 117 pas, pas de champ numérique. Ctrl+S, Entrée, Échap : rien. Lance Large-v3 par erreur : pas d'annulation malgré `cancelModelDownload` dans les bindings. Fait défiler à la molette : la barre latérale reste sur « Audio ». Deux cartes par écran.

**Jordan (primo-arrivant)** — Clique sur « Mouse5 », lit « Press any key... », appuie sur Entrée pour valider → le raccourci devient « Enter ». « Moteur STT (auto-hébergé) », « Direct (wtype/xdotool) », « Déchargement » sont opaques. « Whisper Large-v3 - 3100 MB » : installé ou pas ? Un ⚠️ l'envoie « ci-dessous » vers 8 lignes dont deux « Recommandé » et six « Télécharger », dont deux échoueront (« bientôt »). Enregistre : la fenêtre disparaît, aucune confirmation.

**Sam (clavier + lecteur d'écran)** — Ne peut **pas** modifier le raccourci (P0 pour Sam). Tab dans la barre : « Audio, bouton », sans « courant », sans anneau visible. Aucun `h2` : impossible de sauter entre sections. « Microphone » annoncé « combobox HD-Audio Generic » sans nom ; switch GPU « interrupteur, activé » sans nom ; curseur « 48 » sans nom ; trois boutons de barre de fenêtre anonymes.

**Seb (francophone, dicte en plein travail, ouvre les Paramètres pour un réglage précis)** — Tire le curseur du seuil : atterrit sur 29 ou 31 s, doit flécher. Si la fenêtre avait été fermée avec un changement non enregistré, il voit une valeur qui n'est pas celle du backend. Il lit « 3100 MB », « 8.0 GB », « Error: », « Press any key... » dans une UI qui se dit française. Le micro affiché n'est pas forcément le sien. Après un changement de modèle + Enregistrer, son prochain appui PTT tombe sur un moteur en cours de chargement, sans avertissement.

## Minor Observations

- « KaraokeTyper v1.5.0 » codé en dur (`:1623`) — l'app est en 1.6.0 → `getVersion()` ; « Dictée vocale hors-ligne pour Linux » alors que l'app est multi-plateforme.
- Titre de fenêtre « KaraokeTyper Settings » (anglais, visible dans Alt-Tab, `tauri.conf.json`) ; pas de `minWidth`/`minHeight`.
- « Télécharger les modèles Whisper GGML » (`:1202`) alors que la liste contient Parakeet, SenseVoice, Moonshine.
- Progression : `downloaded_mb` / `total_mb` arrivent dans l'événement (`:48-54`) mais seul le pourcentage est affiché (`:1232`).
- Le dialogue de confirmation nomme `modelName` interne (« ggml-large-v3 ») et « 3100 MB » (`:348`).
- Noms de périphériques longs : `line-clamp-1` sur le trigger, mais les `SelectItem` ne tronquent pas → popover plus large que la fenêtre.
- « Mode actuel : Fonctionne normalement » (`:1118`) est du remplissage ; bouton désactivé « Configuré ✓ » utilisé comme indicateur (`:1289-1300`).
- `UpdateChecker.tsx:105-108` : emerald-500 / red-500 hors tokens.
- Champ de saisie en 16 px (`text-base`) : quatrième taille, contraire à la Three Sizes Rule (constat du détecteur `flat-type-hierarchy`).
- Barre de titre `h-10 px-3` sans padding vertical explicite (`cramped-padding`, mineur).

## Questions to Consider

1. Pourquoi un écran qu'on ouvre « ponctuellement pour un réglage précis » demande-t-il 10 sections et deux écrans de défilement, plutôt que l'écran unique et dense des préférences Raycast que PRODUCT.md cite en référence ?
2. Puisque l'autostart, les permissions et l'extension GNOME s'appliquent déjà immédiatement, qu'est-ce qui empêche d'appliquer *tout* immédiatement et de supprimer « Enregistrer et fermer » — avec un seul point d'attention : le rechargement du modèle ?
3. Que perd « le pupitre de régie » si on retire les dix couleurs d'icônes ? Si la réponse est « rien », elles n'étaient pas de l'identité ; si c'est « on ne distingue plus les sections », la structure des cartes est trop faible pour porter seule la hiérarchie.
4. Les critères de succès sont la latence, la fiabilité du raccourci et la confiance (« quel moteur tourne, où sont mes données ») : pourquoi aucun de ces faits — moteur de la dernière dictée, latence mesurée, chemin et poids des modèles — n'apparaît dans la seule fenêtre où l'utilisateur vient les chercher ?
