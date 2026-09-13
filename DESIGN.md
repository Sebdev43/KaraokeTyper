---
name: KaraokeTyper
description: Dictée vocale 100 % hors-ligne — fenêtre Paramètres et overlay d'enregistrement d'un outil de pro
colors:
  indigo-signal: "#5052e2"
  indigo-signal-on: "#fafafa"
  ink-floor: "#121212"
  ink-panel: "#1a1a1a"
  ink-raised: "#242424"
  ink-line: "#2e2e2e"
  chalk: "#f2f2f2"
  chalk-muted: "#a3a3a3"
  alarm-red: "#ef4444"
  alarm-red-soft: "#f87171"
  live-green: "#34d399"
  wait-amber: "#f59e0b"
  glass-black: "#000000d1"
  glass-edge: "#ffffff14"
typography:
  title:
    fontFamily: "-apple-system, BlinkMacSystemFont, 'Segoe UI', Roboto, 'Helvetica Neue', Arial, sans-serif"
    fontSize: "1.125rem"
    fontWeight: 600
    lineHeight: 1
  body:
    fontFamily: "-apple-system, BlinkMacSystemFont, 'Segoe UI', Roboto, 'Helvetica Neue', Arial, sans-serif"
    fontSize: "0.875rem"
    fontWeight: 400
    lineHeight: 1.5
  label:
    fontFamily: "-apple-system, BlinkMacSystemFont, 'Segoe UI', Roboto, 'Helvetica Neue', Arial, sans-serif"
    fontSize: "0.875rem"
    fontWeight: 500
    lineHeight: 1
  caption:
    fontFamily: "-apple-system, BlinkMacSystemFont, 'Segoe UI', Roboto, 'Helvetica Neue', Arial, sans-serif"
    fontSize: "0.75rem"
    fontWeight: 400
    lineHeight: 1.5
  hud:
    fontFamily: "-apple-system, BlinkMacSystemFont, 'Segoe UI', Roboto, sans-serif"
    fontSize: "13px"
    fontWeight: 500
    lineHeight: 1
rounded:
  control: "6px"
  panel: "12px"
  hud: "14px"
  pill: "9999px"
spacing:
  xs: "4px"
  sm: "8px"
  md: "16px"
  lg: "24px"
components:
  button-primary:
    backgroundColor: "{colors.indigo-signal}"
    textColor: "{colors.indigo-signal-on}"
    rounded: "{rounded.control}"
    padding: "8px 16px"
    height: "36px"
    typography: "{typography.label}"
  button-primary-hover:
    backgroundColor: "#5052e2e6"
  button-secondary:
    backgroundColor: "{colors.ink-raised}"
    textColor: "{colors.indigo-signal-on}"
    rounded: "{rounded.control}"
    padding: "8px 16px"
    height: "36px"
  button-ghost:
    backgroundColor: "#00000000"
    textColor: "{colors.chalk}"
    rounded: "{rounded.control}"
    padding: "8px 16px"
    height: "36px"
  button-ghost-hover:
    backgroundColor: "{colors.ink-raised}"
  button-destructive:
    backgroundColor: "{colors.alarm-red}"
    textColor: "{colors.chalk}"
    rounded: "{rounded.control}"
    padding: "8px 16px"
    height: "36px"
  input:
    backgroundColor: "#2e2e2e4d"
    textColor: "{colors.chalk}"
    rounded: "{rounded.control}"
    padding: "4px 12px"
    height: "36px"
    typography: "{typography.body}"
  select-trigger:
    backgroundColor: "#2e2e2e4d"
    textColor: "{colors.chalk}"
    rounded: "{rounded.control}"
    padding: "8px 12px"
    height: "36px"
  card:
    backgroundColor: "{colors.ink-panel}"
    textColor: "{colors.chalk}"
    rounded: "{rounded.panel}"
    padding: "24px"
  badge-on:
    backgroundColor: "{colors.indigo-signal}"
    textColor: "{colors.indigo-signal-on}"
    rounded: "{rounded.pill}"
    padding: "2px 8px"
    typography: "{typography.caption}"
  badge-off:
    backgroundColor: "{colors.ink-raised}"
    textColor: "{colors.indigo-signal-on}"
    rounded: "{rounded.pill}"
    padding: "2px 8px"
    typography: "{typography.caption}"
  nav-item:
    backgroundColor: "#00000000"
    textColor: "{colors.chalk-muted}"
    padding: "8px 16px"
    typography: "{typography.body}"
  nav-item-active:
    backgroundColor: "#5052e21a"
    textColor: "{colors.chalk}"
    padding: "8px 16px"
    typography: "{typography.label}"
  hud-pill:
    backgroundColor: "{colors.glass-black}"
    textColor: "#ffffffbf"
    rounded: "{rounded.hud}"
    padding: "10px 18px"
    typography: "{typography.hud}"
---

# Design System: KaraokeTyper

## 1. Overview

**Creative North Star: "Le pupitre de régie"**

KaraokeTyper se règle comme une console d'ingénieur du son : des surfaces mates et sombres, un seul voyant d'accent, et chaque réglage étiqueté pour être lu d'un coup d'œil pendant qu'on fait autre chose. L'interface existe pour choisir un micro, un raccourci, un moteur, puis disparaître ; l'overlay d'enregistrement est le seul élément que l'utilisateur voit au quotidien, et il doit informer sans réclamer d'attention. La densité est celle d'un outil de pro : compacte mais respirée, jamais tassée.

Le système rejette explicitement l'« app IA » tape-à-l'œil (dégradés violets, verre décoratif, badges ✨, copy marketing), le panneau de configuration brut sans hiérarchie ni aide, et la navigation arc-en-ciel qui colore chaque section différemment sans porter d'information. Le thème est sombre uniquement, en français.

**Key Characteristics:**
- Fond quasi noir tonal (#121212 → #1a1a1a → #242424), pas de dégradé, pas de texture.
- Un accent unique, l'indigo signal, réservé à l'interaction : bouton principal, focus, état actif, curseur.
- Police système, hiérarchie portée par la graisse et la taille (12 / 14 / 18 px), pas de display.
- Contrôles nets et discrets : coins de 6 px, hauteur 36 px, relief nul, anneau de focus de 3 px.
- Cartes de réglage à coins de 12 px, bord de 1 px, ombre à peine perceptible.
- Overlay HUD en verre noir flouté, pilule de 14 px, voyants vert / rouge / ambre : le seul verre autorisé.

## 2. Colors: La console sombre

Une échelle de gris neutre (sans teinte) sur laquelle un seul indigo fait office de voyant ; trois couleurs d'état (vert, rouge, ambre) n'existent que dans l'overlay et les alertes.

### Primary
- **Indigo signal** (#5052e2 — `hsl(239 72% 60%)`, `--color-primary`, `--color-accent`, `--color-ring` ; 5,5:1 sous #fafafa, 3:1 sur ink panel) : le voyant. Bouton principal, fond de l'interrupteur activé, piste et poignée du curseur, bord droit de 2 px et fond à 10 % de l'entrée de navigation active, anneau de focus à 50 %, badge « actif / installé ». Il n'apparaît jamais en aplat décoratif ni dans un dégradé.
- **Indigo signal on** (#fafafa, `--color-primary-foreground`) : texte posé sur l'indigo.

### Neutral
- **Ink floor** (#121212, `--color-background`) : fond de la fenêtre.
- **Ink panel** (#1a1a1a, `--color-card`, `--color-popover`) : cartes de réglage, menus déroulants.
- **Ink raised** (#242424, `--color-secondary`, `--color-muted`) : boutons secondaires, survol des boutons fantômes, fond de la barre latérale à 30 %.
- **Ink line** (#2e2e2e, `--color-border`, `--color-input`) : bordures de carte, bord des champs, séparateurs ; fond des champs à 30 %.
- **Chalk** (#f2f2f2, `--color-foreground`) : texte principal.
- **Chalk muted** (#a3a3a3, `--color-muted-foreground`) : descriptions, entrées de navigation inactives, icônes au repos. Contraste 6,9:1 sur ink panel.

### Status (overlay et alertes uniquement)
- **Alarm red** (#ef4444, `--color-destructive`) : bouton destructif, voyant « enregistrement », barres du vumètre ; **Alarm red soft** (#f87171) pour le texte d'alerte sur fond sombre (contraste 5,9:1).
- **Live green** (#34d399) : voyant « prêt » de l'overlay, avec halo `0 0 6px rgba(52,211,153,0.4)`.
- **Wait amber** (#f59e0b / texte #fbbf24) : « transcription en cours », éclair pulsant.
- **Glass black** (rgba(0,0,0,0.82)) et **Glass edge** (rgba(255,255,255,0.08)) : la pilule de l'overlay, et rien d'autre.

### Named Rules
**The One Signal Rule.** Un seul accent sur tout écran de réglage : l'indigo. Une section ne possède pas de couleur ; l'identité vient de son titre et de son icône en chalk muted (la barre latérale arc-en-ciel a été retirée le 2026-09-13 ; ne pas la réintroduire).

**The Status Means Status Rule.** Vert, rouge et ambre ne disent qu'une chose : l'état d'un enregistrement ou d'une installation. Jamais pour décorer une icône de section ni un titre.

## 3. Typography

**Display Font:** aucun — le système n'a pas de titre d'affichage.
**Body Font:** pile système (`-apple-system, BlinkMacSystemFont, "Segoe UI", Roboto, "Helvetica Neue", Arial, sans-serif`).
**Label/Mono Font:** identique ; pas de monospace.

**Character:** la police du système d'exploitation, telle quelle : l'application se fond dans le bureau et ne réclame aucune identité typographique. La hiérarchie se joue sur trois tailles et deux graisses.

### Hierarchy
- **Title** (600, 18 px, line-height 1) : titre de carte de réglage, précédé d'une icône de 20 px. Un par carte.
- **Label** (500, 14 px, line-height 1) : nom d'un réglage, entrée de navigation active, titre de la barre de fenêtre.
- **Body** (400, 14 px, line-height 1.5) : texte des contrôles, entrées de navigation inactives, description sous un interrupteur (en chalk muted).
- **Caption** (400, 12 px, line-height 1.5) : aide sous un contrôle, badge, alerte (en chalk muted ou alarm red soft).
- **HUD** (500, 13 px, blanc à 75 %) : texte de l'overlay, une ligne, jamais de retour.

### Named Rules
**The Three Sizes Rule.** 12, 14 et 18 px, rien d'autre dans la fenêtre Paramètres. Un besoin de « plus gros » est un besoin de plus de graisse ou de plus d'air, pas d'une quatrième taille.

**The One Sentence Rule.** Une description de réglage tient en une phrase qui explique le *pourquoi* ; le *comment* technique (evdev, uinput, GGML) va dans une seconde phrase courte ou nulle part.

## 4. Elevation

Plat par défaut. La profondeur vient des trois paliers de gris (floor → panel → raised) et d'une bordure de 1 px en ink line ; les ombres sont si légères qu'elles se lisent comme un liseré, pas comme une élévation. Le seul relief franc est l'overlay d'enregistrement : une pilule de verre noir flouté posée **au-dessus des autres applications**, où le flou a une fonction (rester lisible sur n'importe quel fond) et non un rôle décoratif.

### Shadow Vocabulary
- **Card rest** (`box-shadow: 0 1px 2px 0 rgb(0 0 0 / 0.05)`, `shadow-sm`) : cartes de réglage, imperceptible sur fond sombre — la bordure fait le travail.
- **Control rest** (`box-shadow: 0 1px 2px 0 rgb(0 0 0 / 0.05)`, `shadow-xs`) : champs, déclencheurs de select, interrupteur.
- **Focus ring** (`box-shadow: 0 0 0 3px rgb(80 82 226 / 0.5)` + bordure indigo) : tout contrôle au focus clavier ; c'est le seul « halo » autorisé dans la fenêtre.
- **HUD** (`backdrop-filter: blur(12px)`, `box-shadow: 0 2px 8px rgba(0,0,0,0.3)`, bordure glass edge) : l'overlay, et seulement lui.

### Named Rules
**The Glass Is a HUD Rule.** `backdrop-filter` n'existe que dans `overlay.css`. Une carte, un menu ou un bouton flouté est une faute.

**The Border Does the Lifting Rule.** Sur fond sombre, une ombre de 16 px de flou n'ajoute que de la boue. Une bordure de 1 px en ink line et un palier de gris suffisent à séparer deux surfaces.

## 5. Components

Nets et discrets : coins de 6 px, 36 px de haut, aucun relief, l'accent n'apparaît qu'au focus ou à l'état actif. Tout est construit sur shadcn/ui (style *new-york*, base zinc) avec Radix, icônes lucide à 16 px.

### Buttons
- **Shape :** coins légèrement arrondis (6 px, `rounded-md`), 36 px de haut, 16 px de marge horizontale, texte 14 px 500, icône 16 px avec 8 px d'écart.
- **Primary :** fond indigo signal, texte #fafafa (5,5:1) ; survol à 90 % d'opacité. Un seul par vue, « Enregistrer et fermer » en bas à droite ; les « Télécharger » sont en outline.
- **Secondary :** fond ink raised, texte #fafafa ; survol à 80 %.
- **Ghost :** transparent, texte chalk ; survol fond ink raised. Boutons de la barre de fenêtre (28 px, icône 14 px) ; la croix de fermeture passe en alarm red au survol.
- **Outline :** bord ink line, fond ink line à 30 %, survol à 50 %.
- **Destructive :** fond alarm red à 60 % (thème sombre), texte blanc.
- **Focus :** bordure indigo + anneau de 3 px à 50 %. **Disabled :** opacité 50 %, pointeur inerte.

### Badges
- **Style :** pilule (rayon plein), 8 px de marge horizontale, 2 px verticale, texte 12 px 500, icône 12 px.
- **État :** `default` (indigo signal) = installé / actif / disponible ; `secondary` (ink raised) = absent / inactif. Le badge porte toujours un mot, jamais une couleur seule.

### Cards / Containers
- **Corner Style :** 12 px (`rounded-xl`).
- **Background :** ink panel sur ink floor.
- **Shadow Strategy :** card rest (voir Elevation) ; c'est la bordure qui délimite.
- **Border :** 1 px ink line.
- **Internal Padding :** 24 px ; en-tête avec 12 px sous le titre (`pb-3`) ; 16 px entre réglages (`space-y-4`) ; 8 px entre un libellé et son contrôle (`space-y-2`) ; 24 px entre cartes.
- **Titre :** icône 20 px en chalk muted + texte 18 px 600.

### Inputs / Fields
- **Style :** bord 1 px ink line, fond ink line à 30 %, 6 px de rayon, 36 px de haut, 12 px de marge horizontale, texte 14 px, placeholder chalk muted.
- **Focus :** bordure indigo + anneau 3 px à 50 %, transition sur couleur et ombre.
- **Error :** bordure alarm red + anneau rouge à 40 % (`aria-invalid`). **Disabled :** opacité 50 %.
- **Select :** même déclencheur que le champ, chevron lucide en chalk muted ; menu en ink panel, 6 px de rayon, élément survolé en ink raised.
- **Switch :** piste 32 × 18 px pilule, ink line à 80 % au repos, indigo signal activé ; pouce 16 px chalk (repos) ou #fafafa (activé), translation animée.
- **Slider :** piste 6 px de haut indigo à 20 %, remplissage indigo, poignée 16 px fond ink floor bordée indigo à 50 % ; `Home` / `End` / flèches au clavier.

### Navigation
- **Style :** barre latérale fixe de 192 px, fond ink raised à 30 %, bord droit 1 px ; entrées 14 px avec icône 16 px, 16 px de marge horizontale, 8 px verticale.
- **Default :** texte chalk muted. **Hover :** texte chalk, fond ink raised à 50 %. **Active :** texte chalk 500, fond indigo à 10 %, bord droit 2 px indigo.
- **Mobile :** sans objet (fenêtre desktop 820 × 620, redimensionnable).
- **Barre de fenêtre :** 36 px, sans décorations système, zone de glissement, icône Settings en indigo, titre 14 px 500, trois boutons fantômes de 28 px.

### Recording HUD (signature)
La pilule d'overlay : fond glass black, bord glass edge, 14 px de rayon, 10 × 18 px de marge, flou de 12 px, apparition en fondu 300 ms. À gauche un voyant de 8 px — vert « prêt » (halo vert), rouge pulsant « enregistrement » (1,2 s), ambre pulsant avec éclair « transcription » — puis un vumètre de barres de 3 px (vert/rouge/ambre) et un texte 13 px blanc à 75 %. Une seule ligne, jamais de bouton.

## 6. Do's and Don'ts

### Do:
- **Do** réserver l'indigo signal (#5052e2) à l'interaction : bouton principal, focus, actif, curseur, badge « installé ». S'il couvre plus de 10 % d'un écran, c'est trop.
- **Do** utiliser les paliers ink floor / panel / raised (#121212 / #1a1a1a / #242424) et une bordure de 1 px ink line (#2e2e2e) pour séparer les surfaces.
- **Do** tenir la hiérarchie à trois tailles (12 / 14 / 18 px) et deux graisses (400 / 500-600).
- **Do** rendre chaque état visible **et** lisible : un badge « Installé » vert n'existe pas, un badge indigo « Installé » oui ; une alerte rouge porte toujours un texte qui dit quoi faire.
- **Do** garder les contrôles à 36 px de haut, 6 px de rayon, anneau de focus de 3 px ; tout se règle au clavier (Tab, flèches, Home/End).
- **Do** écrire les libellés en français clair, avec une phrase de *pourquoi* ; les termes techniques (evdev, uinput, GGML, ONNX) ne sont jamais seuls.
- **Do** respecter `prefers-reduced-motion` : les pulsations de l'overlay et les transitions deviennent statiques.

### Don't:
- **Don't** reproduire l'« app IA » tape-à-l'œil de PRODUCT.md : pas de dégradé violet, pas de verre décoratif, pas de badge ✨, pas de copy « propulsé par l'IA ».
- **Don't** retomber dans le panneau de configuration brut : un réglage sans libellé de *pourquoi*, un formulaire gris sans cartes, une erreur muette.
- **Don't** étendre la navigation arc-en-ciel : aucune nouvelle section ne reçoit une teinte `*-400` ; l'icône de section est en chalk muted, l'accent revient à l'état actif.
- **Don't** utiliser `backdrop-filter` hors de `overlay.css`, ni une ombre de plus de 8 px de flou dans la fenêtre Paramètres : sur fond sombre, ça lit comme de la boue (« si ça ressemble à une app de 2014, l'ombre est trop sombre et le flou trop court »).
- **Don't** ajouter une quatrième taille de texte, une police d'affichage ou un dégradé de texte.
- **Don't** porter une information par la couleur seule (vert = ok) : le mot accompagne toujours la couleur.
- **Don't** faire dépasser 24 px le rayon d'une carte ni poser une bande colorée de plus de 2 px sur un bord (le bord droit de 2 px de la navigation active est la seule exception, et elle reste indigo).
