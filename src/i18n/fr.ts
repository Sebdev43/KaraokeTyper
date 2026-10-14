/**
 * French catalogue — the reference. `en.ts` is typed against it, so a key added
 * here without a translation there is a build error rather than a French
 * sentence appearing in an English window.
 *
 * Values that need a runtime value are functions, not templates assembled at the
 * call site: a translator has to be able to move the placeholder inside the
 * sentence, which `"Volume (" + n + "%)"` in a component makes impossible.
 */
export const fr = {
  common: {
    cancel: "Annuler",
    delete: "Supprimer",
    error: "Erreur",
    success: "Succès",
    install: "Installer",
    installing: "Installation…",
    retry: "Réessayer",
    test: "Tester",
  },
  window: {
    title: "KaraokeTyper · Paramètres",
    minimize: "Réduire la fenêtre",
    maximize: "Agrandir la fenêtre",
    closeWithoutSaving: "Fermer sans enregistrer",
    saveAndClose: "Enregistrer et fermer",
    sectionsNav: "Sections des paramètres",
  },
  nav: {
    interface: "Interface",
    audio: "Audio",
    shortcut: "Raccourci",
    stt: "Reconnaissance",
    performance: "Performance",
    models: "Modèles",
    system: "Système",
    terminal: "Terminal",
    sounds: "Sons",
    unload: "Déchargement",
    ollama: "Ollama",
    about: "À propos",
  },
  save: {
    saving: "Enregistrement…",
    savingAndLoading: (model: string) =>
      `Enregistrement, puis chargement de ${model} (quelques secondes)…`,
    savedAndLoaded: (model: string) => `Enregistré, ${model} chargé.`,
    saved: "Paramètres enregistrés.",
    failed: (reason: string) => `Erreur : ${reason}`,
  },
  uiLanguage: {
    title: "Langue de l'interface",
    label: "Langue des fenêtres et du menu",
    help: "Indépendante de la langue dictée, réglée dans « Reconnaissance vocale ». Le changement est visible aussitôt et conservé à l'enregistrement.",
    french: "Français",
    english: "English (anglais)",
  },
  audio: {
    title: "Entrée audio",
    microphone: "Microphone",
    placeholder: "Sélectionner un micro",
    refresh: "Actualiser la liste des micros",
  },
  shortcut: {
    title: "Raccourci clavier",
    key: "Touche d'enregistrement",
    capturing: "Appuyez sur une touche ou un bouton souris…",
    unset: "Non défini",
    change: "Modifier",
    hintCapturing:
      "En attente d'une touche ou d'un bouton souris (Mouse4, Mouse5…). Échap pour annuler.",
    hintIdle: "Cliquez sur Modifier, puis appuyez sur la combinaison voulue.",
    mouseLinuxOnly:
      "Un bouton de souris ne déclenche l'enregistrement que sous Linux ; ailleurs, choisissez une touche.",
    activationMode: "Mode d'activation",
    hold: "Maintenir",
    holdHelp: "Enregistre tant que la touche est maintenue ; relâchez pour transcrire.",
    toggle: "Basculer",
    toggleHelp: "Un appui démarre l'enregistrement, le suivant l'arrête.",
    holdOrToggle: "Maintenir ou basculer",
    holdOrToggleHelp:
      "Appui long : enregistre tant que la touche est enfoncée. Appui bref : enregistre jusqu'au prochain appui.",
    holdThreshold: (ms: number) => `Seuil de maintien (${ms} ms)`,
    holdThresholdHelp: (defaultMs: number) =>
      `Relâché avant ce délai, l'appui est une bascule ; après, un maintien (${defaultMs} ms par défaut).`,
  },
  stt: {
    title: "Reconnaissance vocale",
    backend: "Moteur STT (auto-hébergé)",
    backendWhisper: "Whisper · généraliste, 99 langues",
    backendParakeet: "Parakeet V3 · FR + 24 langues UE, très précis (recommandé)",
    backendSenseVoice: "SenseVoice · multilingue (FR/EN/ES/DE), bientôt",
    backendMoonshine: "Moonshine · ultra-léger (EN), bientôt",
    backendHelp:
      "100 % local. Parakeet V3 auto-détecte la langue dans son set de 25. Whisper accepte une langue explicite.",
    activeModel: "Modèle actif",
    comingSoonSuffix: " (bientôt)",
    notInstalledSuffix: " (Non installé)",
    modelMissing: (model: string) =>
      `Le modèle "${model}" n'est pas installé. Téléchargez-le dans "Modèles" ci-dessous.`,
    orphansHint: (count: number, sizeMb: number) =>
      count === 1
        ? `Un ancien fichier de modèle occupe encore ${sizeMb} Mo : l'encart « Fichiers inutilisés » de la section « Modèles » ci-dessous permet de le supprimer.`
        : `${count} anciens fichiers de modèle occupent encore ${sizeMb} Mo : l'encart « Fichiers inutilisés » de la section « Modèles » ci-dessous permet de les supprimer.`,
    hybrid: "Parakeet pour les dictées courtes",
    hybridHelp:
      "Parakeet V3 est bien plus rapide que Whisper : il transcrit les dictées courtes, Whisper reprend la main au-delà du seuil.",
    hybridWhisperOnly: "Disponible uniquement avec Whisper comme moteur principal.",
    hybridParakeetMissing:
      "Parakeet V3 n'est pas installé : toutes les dictées passent par Whisper jusqu'à son téléchargement dans \"Modèles\" ci-dessous.",
    hybridThreshold: (seconds: number) => `Bascule vers Whisper au-delà de ${seconds} s`,
    spokenLanguage: "Langue dictée",
    spokenLanguageHelp:
      "La langue que le moteur transcrit. Elle n'a aucun effet sur la langue de cette fenêtre.",
  },
  performance: {
    title: "Performance",
    gpuDetected: "GPU détecté",
    noGpu: "Aucun GPU détecté",
    detecting: "Détection...",
    cpuOnly: "CPU seul",
    vramSuffix: (amount: string) => ` (${amount} VRAM)`,
    recommendedModel: "Modèle max recommandé pour votre GPU :",
    vramWarning: (model: string, recommended: string) =>
      `Votre modèle actuel (${model}) risque de dépasser la capacité VRAM et provoquer des crashs. Essayez ${recommended}.`,
    currentMode: "Mode actuel",
    runningNormally: "Fonctionne normalement",
    gpuActive: "GPU actif",
    cpuMode: "Mode CPU",
    gpuDisabledAfter: (count: number) => `GPU désactivé après ${count} échecs`,
    gpuFailures: (count: number) => `${count} échec(s) GPU enregistré(s)`,
    gpuDisabledHelp: "Le GPU ne sera pas utilisé tant que vous ne réinitialisez pas le compteur",
    gpuFailuresHelp: "Le GPU sera désactivé après 3 échecs consécutifs",
    gpuToggle: "Accélération GPU",
    gpuOn: "GPU utilisé pour la transcription",
    gpuOff: "CPU utilisé (plus compatible)",
    gpuUnavailable:
      "Accélération GPU non disponible. Compilé sans support GPU ou aucun GPU compatible détecté.",
    gpuResetTitle: "Réinitialisation GPU",
  },
  models: {
    title: "Gestion des modèles",
    intro:
      "Télécharger les modèles de reconnaissance vocale. Whisper est au format GGUF depuis la 1.9.0 : un fichier installé avant n'est plus lisible.",
    orphansTitle: "Fichiers inutilisés",
    orphansReclaimable: (sizeMb: number) => `${sizeMb} Mo récupérables`,
    orphansHelp:
      "Plus aucun modèle du catalogue n'utilise ces fichiers : format précédent, téléchargement interrompu. Rien n'est supprimé sans votre geste.",
    deleteFailed: (reason: string) => `Suppression impossible : ${reason}`,
    installed: "Installé",
    recommended: "Recommandé",
    comingSoon: "Bientôt",
    download: "Télécharger",
    downloading: (percent: string) => `Téléchargement… ${percent}%`,
    confirmTitle: "Télécharger un modèle",
    confirmBody: (model: string, sizeMb: number) =>
      `Télécharger le modèle ${model} (${sizeMb} MB) ?\n\nCela peut prendre plusieurs minutes selon votre connexion.`,
    downloadFailed: (reason: string) => `Échec du téléchargement : ${reason}`,
    sizeMb: (sizeMb: number) => `${sizeMb} Mo`,
  },
  system: {
    title: "Configuration système",
    permissions: "Permissions système",
    permissionsOk: "Permissions système configurées",
    permissionsMissing: "Configurer les permissions evdev/uinput",
    active: "Actif",
    missing: "Manquant",
    pushToTalk: "Appui pour parler (evdev)",
    typing: "Saisie de texte (uinput)",
    configured: "Configuré",
    configurePermissions: "Configurer les permissions",
    confirmInstallTitle: "Configuration KaraokeTyper",
    confirmInstallBody:
      "Configurer les permissions système ?\n\nCela va :\n• Configurer evdev pour le push-to-talk\n• Installer xdotool (saisie de texte)\n• Installer xclip (clipboard pour RustDesk/Remmina)\n• Peut nécessiter le redémarrage de l'application",
    installFailed: (reason: string) => `Échec de l'installation : ${reason}`,
    removeFailed: (reason: string) => `Échec de la suppression : ${reason}`,
    autostartFailed: (reason: string) => `Échec du changement d'autostart : ${reason}`,
    pasteMethod: "Méthode de collage",
    pasteMethodHelp: "Comment le texte est injecté dans l'application active",
    pasteDirect: "Direct (wtype/xdotool), recommandé sur Linux",
    pasteCtrlV: "Ctrl+V (presse-papiers)",
    pasteCtrlShiftV: "Ctrl+Shift+V (terminaux)",
    pasteShiftInsert: "Shift+Insert (tmux, certains terminaux)",
    pasteNone: "Désactivé",
    autostart: "Lancement au démarrage",
    autostartHelp: "Démarrer automatiquement KaraokeTyper à la connexion",
    autostartOn: "Démarre à la connexion",
    autostartOff: "Démarrage manuel",
  },
  gnome: {
    title: "Détection de terminal",
    intro:
      "Extension GNOME Shell pour la détection automatique des terminaux. Utilise Ctrl+Maj+V dans les terminaux et Ctrl+V ailleurs.",
    active: "Active",
    installedDisabled: "Installée (désactivée)",
    notInstalled: "Non installée",
    autoDetection: "Détection automatique active",
    fallbackDetection: "Détection de secours utilisée",
    installed: "Installée",
    installExtension: "Installer l'extension",
    confirmInstallTitle: "Extension Terminal",
    confirmInstallBody:
      "Installer l'extension GNOME pour la détection de terminal ?\n\nCela permet de coller automatiquement avec Ctrl+Shift+V dans les terminaux.\n\nNote : la session sera redémarrée automatiquement après l'installation.",
    confirmRemoveTitle: "Supprimer l'extension",
    confirmRemoveBody: "Supprimer l'extension de détection de terminal ?",
    notEnabled:
      "Extension installée mais non activée. Activez-la dans l'application Extensions GNOME ou lancez : gnome-extensions enable focused-window-dbus@karaoketyper",
  },
  sounds: {
    title: "Sons de l'application",
    notifications: "Notifications sonores",
    notificationsHelp: "Jouer un son au début et à la fin de l'enregistrement",
    mute: "Couper le son",
    unmute: "Rétablir le son",
    volume: (percent: number) => `Volume (${percent} %)`,
  },
  unload: {
    title: "Déchargement du modèle",
    label: "Décharger après inactivité",
    help: "Libérer la mémoire en déchargeant le modèle STT après une période d'inactivité. Le modèle se recharge automatiquement.",
    never: "Jamais",
    min5: "Après 5 minutes",
    min15: "Après 15 minutes",
    hour1: "Après 1 heure",
  },
  ollama: {
    title: "Intégration Ollama",
    correction: "Correction de texte",
    correctionHelp:
      "Utiliser un LLM local pour corriger la grammaire et l'orthographe après la transcription",
    serverUrl: "URL du serveur",
    model: "Modèle",
    connected: "Connecté",
    disconnected: "Déconnecté",
    ready: (model: string) => `${model} prêt`,
    unreachable: "Ollama non joignable",
  },
  about: {
    title: "À propos",
    tagline: (platform: string) => `Dictée vocale hors-ligne pour ${platform}`,
  },
  updates: {
    title: "Mises à jour",
    subtitle: "Rechercher une nouvelle version sur GitHub",
    check: "Rechercher",
    checking: "Vérification…",
    upToDate: "✓ Vous êtes à jour.",
    error: (reason: string) => `Erreur : ${reason}`,
    installing:
      "Installation en cours… (un prompt de mot de passe peut apparaître pour les installations .deb)",
    availableSuffix: (current: string) => ` disponible (actuel : v${current})`,
    installAndRestart: "Installer et redémarrer",
    openGithub: "Voir la page GitHub",
  },
  main: {
    pressToStart: "Appuyez sur Ctrl+Espace pour commencer",
    ready: "Prêt ! Appuyez sur Ctrl+Espace pour dicter",
    pressToDictate: "Appuyez sur Ctrl+Espace pour dicter",
    listening: "Écoute en cours...",
    transcribing: "Transcription...",
    statusRecording: "Enregistrement",
    statusProcessing: "Traitement",
    statusIdle: "Prêt",
    statusUnknown: "Inconnu",
    stop: "Arrêter",
    record: "Enregistrer",
    lastTranscription: "Dernière transcription :",
  },
  overlay: {
    ready: "Prêt",
    listening: "Écoute...",
    processing: "Traitement",
    ai: "IA...",
  },
};

/**
 * The shape every other catalogue must match, key for key.
 *
 * Deliberately no `as const` on `fr`: with it every value would widen to its own
 * literal type and `const en: Dict` would then demand the *French* text, which is
 * the opposite of the check wanted here.
 */
export type Dict = typeof fr;
