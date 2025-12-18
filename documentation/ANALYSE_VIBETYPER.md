# Analyse de VibeTyper

## Architecture Générale

**Type**: Application Electron (multi-plateforme)
**Version analysée**: 1.0.1
**Localisation**: Extraction AppImage dans `~/Bureau/KaraokeTyper/squashfs-root/`

## Découverte Critique: VibeTyper est Cloud-Based 🚨

Contrairement à ce qu'on pourrait penser, **VibeTyper n'est PAS une solution offline**:

- Envoie l'audio via WebSocket vers des serveurs cloud
- Pas de moteur STT local (pas de Vosk, pas de Whisper)
- Utilise Supabase pour l'authentification et le backend
- Limite gratuite: 1000 mots/mois
- Nécessite une connexion internet pour fonctionner

## Technologies Utilisées

### Backend (package.json)
```json
{
  "dependencies": {
    "@supabase/supabase-js": "^2.58.0",  // Auth + backend cloud
    "axios": "^1.11.0",                   // Requêtes HTTP
    "cacheable-lookup": "^7.0.0",         // Cache DNS
    "koffi": "^2.14.1",                   // FFI pour appels natifs
    "loudness": "^0.4.2",                 // Contrôle volume système
    "uiohook-napi": "^1.5.4"              // Raccourcis clavier globaux ⭐
  }
}
```

### Architecture de Communication
- **WebSocket** pour streaming audio temps réel vers serveurs cloud
- **Supabase** pour authentification et stockage
- **Electron IPC** pour communication main/renderer process

## Fonctionnalités Identifiées

### 1. Commandes Vocales (AI-powered)
Le code obfusqué révèle plusieurs commandes:
- `rewrite` / `re-write` / `re write` - Réécrire du texte
- `reply` / `respond` - Répondre à un message (lit le presse-papier)
- `polish` - Améliorer le texte
- `rephrase` / `re-phrase` - Reformuler
- `improve` - Améliorer
- `compose a reply` - Composer une réponse

### 2. Intégration Presse-papier
Pour la commande "reply", VibeTyper:
1. Lit le contenu du presse-papier via `electron.clipboard.readText()`
2. Envoie le contexte + commande vocale au serveur
3. Génère une réponse via GPT-4
4. Colle le résultat dans le champ actif

### 3. Raccourcis Globaux
Utilise `uiohook-napi` pour capturer les raccourcis même quand l'app n'a pas le focus:
- Push-to-talk (maintenir une touche pour enregistrer)
- Paste on release (coller automatiquement après la reconnaissance)

## Code Source Analysé

### recognizer.js (obfusqué)
```javascript
class SpeechRecognizer {
  constructor() {
    this.apiService = null;
  }

  setApiService(apiService) {
    this.apiService = apiService;
  }

  async processWithAI(input, mode, context) {
    // Envoie vers serveur cloud
    // Mode DEMO si pas connecté
    return { response: aiGeneratedText };
  }

  detectAICommand(text) {
    // Détecte les commandes dans le texte
    // Retourne: { mode: 'rewrite|reply', prompt, clipboardContent }
  }
}
```

### Patterns de Détection
- Commandes sensibles à la casse (converties en lowercase)
- Détection par préfixes: "rewrite: ", "reply to ", etc.
- Correction automatique des erreurs de reconnaissance communes
- Offset pour extraire le contenu après la commande

## Limites et Modèle Commercial

- **Free tier**: 1000 mots/mois
- Nécessite un compte et authentification
- Dépend de la disponibilité des serveurs
- Pas de contrôle sur la confidentialité des données audio
- Pas vraiment "offline" comme annoncé

## Ce qu'on peut apprendre pour KaraokeTyper

### ✅ À reprendre:
1. **Système de raccourcis globaux** - uiohook-napi fonctionne bien
2. **Intégration presse-papier** - Utile pour les commandes contextuelles
3. **Détection de commandes** - Pattern matching intelligent
4. **UX push-to-talk** - Interface simple et intuitive

### ❌ À éviter:
1. **Dépendance cloud** - On veut du 100% local
2. **Code obfusqué** - Rend la maintenance impossible
3. **Modèle freemium** - On veut du libre et gratuit
4. **Electron bloat** - Python + GTK sera plus léger

## Alternative: Notre Approche

Voir `PLAN_KARAOKETYPER.md` pour l'architecture que nous allons implémenter:
- STT local avec Vosk ou faster-whisper
- Pas de serveurs externes
- Open source (GPL v3)
- Interface légère en Python + GTK4
- Support Wayland natif avec ydotool

## Fichiers de Référence

- `/home/seb43/Bureau/KaraokeTyper/squashfs-root/` - Extraction complète
- `resources/app-extracted/package.json` - Dépendances
- `resources/app-extracted/dist/speech/recognizer.js` - Logique reconnaissance (obfusqué)
- `resources/app-extracted/dist/speech/realtimeRecognizer.js` - WebSocket streaming

## Conclusion

VibeTyper est un bon produit commercial, mais pas adapté à nos besoins:
- Pas vraiment offline
- Dépendance cloud
- Limites d'utilisation
- Propriétaire

Notre KaraokeTyper sera **vraiment local, vraiment libre, vraiment offline**.
