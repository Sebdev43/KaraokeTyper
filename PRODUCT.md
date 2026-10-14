# Product

## Register

product

## Users

Public technique multilingue : développeurs et power users qui dictent dans n'importe quelle application (éditeur, terminal, navigateur) via push-to-talk, et qui règlent eux-mêmes micro, raccourci, moteur STT (Whisper / Parakeet), modèles, GPU et post-traitement (Ollama, mots personnalisés). Ils ouvrent la fenêtre Paramètres ponctuellement, pour un réglage précis, souvent en cours de tâche ; ce qu'ils voient le plus est l'overlay pendant l'enregistrement. L'auteur (francophone) est le premier utilisateur au quotidien ; l'app est diffusée par releases GitHub (Linux, Windows, macOS).

## Product Purpose

KaraokeTyper transforme la voix en texte, 100 % hors-ligne, sans cloud ni télémétrie : on maintient une touche ou un bouton souris, on parle, on relâche, le texte apparaît dans l'application active. Le succès se mesure en latence entre le relâchement et le texte tapé, en fiabilité du raccourci, et en confiance : l'utilisateur sait à tout moment quel moteur tourne, où sont ses données, et pourquoi un réglage existe.

## Brand Personality

Sobre, précis, rapide. Voix directe et factuelle, en français, qui explique le « pourquoi » d'un réglage en une phrase et se tait ensuite. Émotion visée : le contrôle tranquille d'un outil de pro qui répond immédiatement. Références : les préférences de Raycast et de Linear pour leur hiérarchie nette, leur accent unique et leur densité maîtrisée.

## Anti-references

- L'app « IA » tape-à-l'œil : dégradés violets, verre décoratif, badges ✨, copy marketing (« propulsé par l'IA »), effets qui promettent plus que l'outil ne fait.
- Le panneau de configuration brut (Windows / GTK) : formulaire gris sans hiérarchie, libellés techniques nus, aucune aide contextuelle, états d'erreur muets.
- La navigation arc-en-ciel : une couleur différente par section qui dilue l'accent et ne porte aucune information.

## Design Principles

1. **Le réglage avant le décor.** Chaque élément à l'écran aide à choisir ou comprendre un réglage ; ce qui ne le fait pas disparaît.
2. **Un accent, une hiérarchie.** Une seule couleur d'interaction ; l'identité d'une section vient de sa structure et de son titre, pas d'une teinte dédiée.
3. **Montrer l'état, pas le promettre.** Modèle installé ou non, GPU actif, moteur qui a transcrit, latence mesurée : des faits visibles, jamais un « prêt » décoratif. Une erreur dit quoi faire ensuite.
4. **Local et honnête.** Dire clairement ce qui tourne où (100 % sur la machine) et ce qu'un réglage coûte (mémoire, temps), sans théâtre.
5. **Mains occupées, clavier d'abord.** L'utilisateur est au milieu d'une tâche : tout se règle au clavier, rien ne bouge inutilement, l'overlay informe sans réclamer d'attention.

## Accessibility & Inclusion

Bases solides, sans certification visée : contraste texte ≥ 4,5:1 sur le thème sombre, navigation clavier complète (Tab, flèches, Home/End sur les curseurs) avec focus visible, respect de `prefers-reduced-motion` (overlay et transitions), aucune information portée par la couleur seule (statut installé / erreur toujours doublé d'un texte ou d'une icône). Libellés en langue claire, en français comme en anglais (Paramètres → Interface), avec `<html lang>` qui suit le choix ; termes techniques (evdev, uinput, GGUF) accompagnés d'une phrase d'explication.
