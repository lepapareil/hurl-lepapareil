# Hurl UI

Client graphique pour [Hurl](https://hurl.dev), sur le modèle de [Bruno](https://www.usebruno.com) :
même disposition (barre latérale des répertoires, onglets de requêtes, panneau requête / panneau
réponse, environnements, runner), mais on y ouvre de simples répertoires de fichiers `.hurl` (il n'y a
pas de notion de collection : Hurl, ce sont des fichiers dans des répertoires) et l'exécution est
confiée au binaire `hurl`.

Hurl UI fait partie du dépôt de Hurl et est livré avec lui, sous le même numéro de version : les
options de `hurl` qu'il propose ne sont pas écrites à la main, elles viennent des fichiers
d'options de Hurl (voir « Options de hurl » ci-dessous).

Application de bureau [Tauri 2](https://tauri.app) (Rust) avec une interface React / TypeScript.
Cibles : Linux, macOS et Windows (voir « État des plateformes » ci-dessous : seul Linux a été
exécuté).

## Correspondance Bruno → Hurl

| Bruno | Hurl UI |
|---|---|
| Collection (`bruno.json` + fichiers `.bru`) | Un dossier quelconque contenant des fichiers `.hurl`, sans fichier de configuration |
| Dossier | Sous-dossier |
| Requête (un fichier `.bru`) | Une entrée d'un fichier `.hurl` (un fichier peut en contenir plusieurs, chaînées) |
| Nom de la requête | Première ligne de commentaire au-dessus de la requête |
| Params / Headers | Section `[Query]` / en-têtes (et `[Cookies]`) |
| Body | Corps JSON, XML, texte multiligne, GraphQL, `[Form]`, `[Multipart]`, `file,…;` ou syntaxe Hurl brute |
| Auth | `[BasicAuth]` ou en-tête `Authorization: Bearer` |
| Vars (post-réponse) | `[Captures]` |
| Assert / Tests | Statut et version attendus, `[Asserts]`, en-têtes et corps attendus |
| Script / Settings | `[Options]` de la requête (pas de script dans Hurl) |
| Docs | Commentaires au-dessus de la requête |
| Environnements | `<nom>.env` à la racine du répertoire ouvert, fichier de variables Hurl (`--variables-file`) |
| Secrets | Hors du répertoire, dans les données de l'application, transmis en secrets Hurl |
| Runner | Exécution de `hurl` fichier par fichier ; le résultat d'un fichier ou d'un répertoire a les mêmes volets que celui d'une requête (Response, Headers, Timeline, Tests), sur toutes les requêtes exécutées |
| Mode « File » | « Edit as Text » : édition du fichier `.hurl` avec coloration syntaxique |
| Coller une commande cURL | Conversion par `hurlfmt --in curl` |

## Prérequis

- `hurl` (et `hurlfmt`, livré avec) de la même version que Hurl UI. Il est cherché à côté de
  l'exécutable de Hurl UI, puis dans le `PATH`, puis dans les répertoires d'installation usuels
  (Homebrew, Cargo, Scoop, WinGet, Chocolatey…), à moins que les préférences n'indiquent son
  chemin. Avec un `hurl` d'une autre version, les préférences le signalent : les options proposées
  restent celles de la version de Hurl UI.
- Pour compiler : la version de Rust de Hurl (`rust-version` de `packages/hurl/Cargo.toml`,
  installée par `bin/install_rust.sh`), Node.js ≥ 20 et les
  [prérequis système de Tauri 2](https://tauri.app/start/prerequisites/)
  (WebKitGTK 4.1 sous Linux, WebView2 et outils MSVC sous Windows, outils Xcode sous macOS).

État des plateformes :

- **Linux (x86_64)** : seule plateforme où l'application a été compilée en entier et exécutée.
  Binaire de release et paquets `.deb` et `.rpm` produits (l'AppImage n'a pas été essayée) ;
  fenêtre native lancée sous Xvfb : affichage des répertoires, prise en compte d'un fichier
  modifié sur le disque, envoi d'une requête au clic, instance unique.
- **Windows et macOS** : seule la compilation du cœur (`hurlui_core`, tests et pont compris) est
  vérifiée, par `cargo check` et `cargo clippy` (sans édition de liens) pour les cibles
  `x86_64-pc-windows-msvc`, `x86_64-pc-windows-gnu`, `aarch64-apple-darwin` et
  `x86_64-apple-darwin`. Rien n'y a été exécuté (ni tests, ni application), et la coquille Tauri
  n'y a pas été compilée : elle demande les outils de la plateforme (compilateur de ressources
  sous Windows, compilateur Objective-C d'Apple sous macOS), donc une compilation sur l'OS cible.

## Utilisation

Hurl UI se compile avec cargo, depuis la racine du dépôt, en même temps que `hurl` et `hurlfmt` :

```sh
cargo build --release --workspace    # target/release/hurl, hurlfmt et hurlui
cargo build --release -p hurlui      # hurlui seul
```

`cargo build` sans `--workspace` ne compile que `hurl` et `hurlfmt` : `hurlui` est un membre du
workspace qui n'est pas compilé par défaut (`default-members` du `Cargo.toml` racine), pour que
compiler Hurl ne demande ni Node.js ni les bibliothèques de la vue web.

L'interface est construite par le script de compilation (`build.rs`) : il lance `npm ci` si les
dépendances ne sont pas installées, puis `npm run build`, et embarque le résultat (`dist/`) dans
le binaire. Node.js doit donc être installé, et `npm ci` a besoin du réseau la première fois. Le
script n'est relancé que si les sources de l'interface changent.

Les paquets de Hurl (archives `tar.gz`, paquet Debian, archive `zip` et installeur Windows)
contiennent les trois binaires : ils sont construits par `bin/release/release.sh --workspace`
(`release.ps1 --workspace` sous Windows). Sous Debian, les bibliothèques de la vue web sont
recommandées (`Recommends`) et non exigées : `hurl` et `hurlfmt` s'installent sans elles.

Pour développer l'interface, avec rechargement à chaud, depuis ce répertoire (`packages/hurlui`) :

```sh
npm ci
npm run tauri dev
```

`tauri dev` compile l'application sans la fonctionnalité `custom-protocol` : l'interface n'est
alors pas embarquée mais servie par Vite.

Raccourcis : `Ctrl/Cmd+Entrée` envoyer, `Ctrl/Cmd+S` enregistrer, `Ctrl/Cmd+W` fermer l'onglet,
`Ctrl/Cmd+\` barre latérale, `Ctrl/Cmd+J` orientation de la réponse, `Ctrl/Cmd+,` préférences.

### Tests

Le cœur (`packages/hurlui_core`) est un membre du workspace de Hurl : ses tests sont lancés par
`cargo test` à la racine du dépôt, avec ceux de Hurl.

```sh
cargo test -p hurlui_core
# Test « oracle » : compare le lecteur/écrivain de fichiers Hurl à hurlfmt sur un corpus,
# par exemple les tests d'intégration de Hurl.
HURL_CORPUS="$PWD/integration" cargo test -p hurlui_core --test corpus -- --nocapture
```

Les tests de `tests/run.rs` pilotent le vrai binaire `hurl` contre un serveur HTTP local ; ils
sont ignorés si `hurl` est absent.

### Mode navigateur (développement de l'interface)

```sh
npm run bridge   # expose l'API du cœur Rust sur http://127.0.0.1:1421 et affiche l'URL à ouvrir
npm run dev      # http://localhost:1420
```

Ouvrir ensuite l'URL affichée par le pont, de la forme
`http://localhost:1420/#bridge-token=<jeton>`. Sans elle, la page n'a aucun accès (elle le dit et
n'affiche aucun répertoire). Le jeton change à chaque démarrage du pont : après un redémarrage,
rouvrir la nouvelle URL.

Le pont est un outil de développement : son API lit et écrit des fichiers et lance des programmes.
Il n'écoute que sur la boucle locale, mais son port (comme celui de Vite) reste joignable par tous
les processus de la machine, y compris ceux d'autres utilisateurs. L'accès repose donc sur un
jeton :

- un jeton aléatoire est généré à chaque lancement. Il n'est connu que de la page ouverte depuis
  l'URL affichée (elle le prend dans le fragment, qui n'est pas envoyé au serveur, le retire de la
  barre d'adresse, le garde dans le `sessionStorage` de l'onglet et l'envoie dans un en-tête de
  chaque requête) et des processus de l'utilisateur, par le fichier `target/dev-bridge.token`
  (droits `0600`), prévu pour l'automatisation et les tests ;
- le proxy de Vite transmet les requêtes de la page et n'ajoute rien : atteindre le port de Vite
  ou celui du pont ne donne aucun accès. Les autres utilisateurs de la machine peuvent joindre ces
  ports mais n'ont pas le jeton ;
- une requête dont l'`Origin` n'est pas le serveur de développement est refusée, même avec le
  jeton.

L'état du pont (`state.json`, `secrets.json`) est séparé de celui de l'application de bureau :
il est dans `target/dev-bridge-config`. Variables d'environnement : `HURLUI_BRIDGE_PORT` (1421,
lue aussi par le proxy de Vite), `HURLUI_CONFIG_DIR`, `HURLUI_BRIDGE_TOKEN_FILE` et
`HURLUI_BRIDGE_ORIGINS` (origines admises, séparées par des virgules ; la première est l'adresse
de l'interface). Dans ce mode, les sélecteurs de dossier sont remplacés par une saisie de chemin.

## Options de hurl

Rien de ce que Hurl UI sait des options de `hurl` n'est écrit dans son code. Chaque option de Hurl
est décrite par un fichier de `docs/spec/options/hurl/*.option`, dont `bin/spec/options/generate_all.py`
tire déjà l'analyseur de la ligne de commande (`clap`), la page de manuel et les complétions des
shells. Le même script produit le catalogue de Hurl UI, `packages/hurlui_core/src/options/hurl.json`
(générateur : `bin/spec/options/generate_ui.py`), embarqué dans l'application. En viennent :

- les **préférences** : une case, une liste ou un champ par option, sous les rubriques de
  `hurl --help`, avec son aide, le nom de sa valeur et sa valeur par défaut ;
- l'**onglet Options** d'une requête : les options admises dans une section `[Options]`, y compris
  celles qui n'existent que là (`skip`), et le type de leur valeur, tous deux lus dans `hurl_core`.
  La valeur se saisit selon ce type : liste `true` / `false` pour un booléen (`true` par défaut),
  liste des valeurs admises pour un choix, chiffres pour un entier, entier et unité pour une durée,
  texte libre sinon. Une valeur qui ne convient pas est signalée ; un `{{variable}}`, que Hurl
  admet pour toute option, reste toujours possible ;
- la **ligne de commande** de `hurl` : le cœur la construit à partir du catalogue et refuse une
  option qu'il ne connaît pas.

Ajouter, renommer ou supprimer une option de Hurl se fait donc comme avant (le fichier `.option`
puis `bin/spec/options/generate_all.py`) et Hurl UI suit. `bin/check/generated.sh`, lancé par la
CI, échoue si le catalogue n'est pas à jour.

Les **opérateurs des asserts** suivent le même principe : la liste proposée est celle des prédicats
de Hurl (chacun pouvant être précédé de `not`), et un test du cœur échoue si elle diffère de celle
de `hurl_core` (`PredicateFuncValue`).

Ce qui reste écrit dans Hurl UI est ce qui lui appartient : les options qu'il fixe lui-même à
chaque exécution (`--report-json`, `--no-output`, `--no-color`, `--from-entry`, `--to-entry`, le
niveau de trace) et celles qui changeraient la sortie qu'il lit. Elles sont désignées par le nom de
leur fichier d'option (`MANAGED` dans `packages/hurlui_core/src/options/mod.rs`), ne sont pas
proposées dans les préférences, et un test échoue si l'une d'elles disparaît de Hurl.

## Architecture

```
packages/hurlui_core/     cœur Rust, indépendant de Tauri (membre du workspace de Hurl)
  src/hurlfile.rs          lecture / écriture des fichiers Hurl (modèle d'une entrée)
  src/collection.rs        répertoires ouverts : arborescence, fichiers, environnements
  src/options/             options de hurl : catalogue généré (hurl.json) et ligne de commande
  src/runner.rs            localisation et exécution de hurl, lecture du rapport JSON
  src/store.rs             état de l'application et secrets (fichiers JSON)
  src/watcher.rs           surveillance du disque
  src/api.rs               répartiteur de commandes JSON
  examples/dev_bridge.rs   pont de développement (mode navigateur)
packages/hurlui/          application de bureau
  src/                     coquille Tauri : une commande `api` + les événements
  build.rs                 construit l'interface (npm) et l'embarque dans le binaire
  ui/                      interface React (zustand, CodeMirror 6)
  icons/                   icônes de l'application, générées à partir du logo de Hurl
  tauri.conf.json          configuration Tauri (la version est celle de Cargo.toml)
```

Hurl UI n'a pas de logo propre : son logo est celui de Hurl, `art/logo-icon.svg`. L'interface
importe ce fichier directement. Les icônes de l'application (`icons/`), que Tauri et les paquets
demandent dans des formats matriciels, en sont générées :

```sh
rsvg-convert -w 1024 -h 1024 art/logo-icon.svg -o /tmp/logo.png
npx tauri icon /tmp/logo.png -o /tmp/icons    # depuis packages/hurlui
# puis copier 32x32.png, 128x128.png, 128x128@2x.png, icon.icns et icon.ico dans icons/
```

Les deux paquets sont des membres du workspace de Hurl : un seul `Cargo.lock`, un seul numéro de
version, mis à jour par les workflows de livraison pour les cinq paquets ensemble. Les dépendances
de Tauri (plusieurs centaines de crates) sont donc dans le `Cargo.lock` de Hurl, et couvertes par
ses contrôles (licences, mise à jour des crates), mais ne sont compilées que si `hurlui` l'est.

Choix structurants :

- **`hurl` reste la référence du langage.** L'application n'embarque pas le parseur de
  Hurl (pas de dépendance à `libxml2`). `hurlfile.rs` se contente de
  localiser les blocs d'une entrée et conserve chaque valeur sous forme de texte source : filtres,
  prédicats ou options inconnus de l'interface sont préservés. La syntaxe est vérifiée par
  `hurlfmt` à l'ouverture et à l'enregistrement, et l'erreur s'affiche dans un bandeau.
- **Édition par épissure.** Enregistrer une requête ne régénère que cette entrée ; le reste du
  fichier est conservé à l'octet près. Le texte généré est relu avant écriture : s'il ne redonne
  pas exactement une entrée (URL vide, statut non numérique…), l'enregistrement est refusé.
- **Les onglets suivent leur requête.** Une requête est repérée par sa position dans le fichier.
  Quand le fichier change (ajout, suppression, édition en texte, autre programme), les onglets et
  leurs résultats sont réalignés sur la requête correspondante. Un onglet contenant des
  modifications non enregistrées n'est jamais fermé d'office : il est signalé « en conflit » (la
  requête a changé sur le disque, l'enregistrement demande confirmation) ou « orphelin » (la
  requête n'existe plus, les modifications restent affichées mais ne peuvent plus être écrites).
- **Exécution.** `hurl <fichier> --report-json <tmp> --no-output [--from-entry N --to-entry N]`,
  suivi des options des préférences.
  Le rapport fournit appels, en-têtes, corps, chronométrage, captures et asserts ; les erreurs
  d'exécution (variable indéfinie, connexion…) sont extraites de la sortie d'erreur et rattachées
  à leur entrée. Le rapport et les corps de réponse de chaque exécution vont dans un répertoire
  temporaire privé (`hurlui-run-…`, droits `0700` sous Unix), supprimé à la fin de l'exécution ;
  ceux qu'une application tuée en cours d'exécution aurait laissés sont supprimés au démarrage
  s'ils ont plus de 24 h (sous Unix, seulement ceux de l'utilisateur).
- **Une seule API.** La coquille Tauri et le pont de développement appellent le même répartiteur
  (`api.rs`), ce qui permet de tester l'interface dans un navigateur contre le vrai cœur.
- **Grands répertoires.** L'arborescence de la barre latérale et la liste des fichiers du runner
  sont des listes fenêtrées : seules les lignes visibles sont montées. Le cœur garde d'un parcours
  du répertoire au suivant le résumé des fichiers inchangés (date de modification et taille) et
  ne relit que les autres ; un fichier modifié depuis moins de 3 s est toujours relu, car les
  horodatages des systèmes de fichiers peuvent être grossiers (jusqu'à 2 s).

## Différences avec Bruno et limites

- **Envoyer enregistre.** `hurl` exécute le fichier présent sur le disque : les modifications en
  cours de la requête sont enregistrées avant l'envoi.
- **Envoi isolé ou chaîné.** Par défaut seule l'entrée affichée est exécutée ; le bouton « chaîne »
  de la barre d'URL exécute d'abord les entrées précédentes du fichier, pour disposer des
  variables capturées.
- Enregistrer une requête depuis le formulaire supprime les commentaires situés *à l'intérieur* de
  cette entrée (ceux qui la précèdent sont sa documentation et sont conservés) et normalise les
  noms de sections (`[Query]`, `[Form]`, `[Multipart]`). « Edit as Text » ne modifie rien d'autre
  que ce qui est saisi.
- Pas de case « activé » par ligne : Hurl n'a pas de notion de paramètre désactivé.
- Dans les formulaires, un `#` saisi dans une URL, un en-tête ou un paramètre est écrit `\#` dans
  le fichier (sinon Hurl y verrait un commentaire) et s'affiche ensuite sous cette forme.
- Dans l'onglet Assert, une version HTTP attendue sans statut s'écrit `HTTP/2 *` dans le fichier.
  Dans l'onglet Tests, la ligne de réponse (`HTTP 200`), pour laquelle Hurl rapporte deux asserts
  (version et statut), occupe une seule ligne quand les deux passent.
- Corps de réponse : 20 Mo affichés par corps (au-delà il est tronqué) et 100 Mo chargés par
  exécution, en commençant par les dernières requêtes ; les corps des autres sont signalés comme
  non chargés, avec leur taille (envoyer la requête seule pour le voir). Le JSON est formaté par
  défaut, quelle que soit sa taille (bascule Pretty / Raw). Un corps binaire n'est prévisualisé
  que si c'est une image de 5 Mo au plus, sinon seule sa taille est indiquée. De la sortie de
  `hurl` (trace `--verbose`), seuls le premier et le dernier Mo sont conservés. Hors des requêtes
  ouvertes dans un onglet, seuls les 50 derniers résultats gardent leur corps en mémoire.
- Un répertoire ouvert momentanément inaccessible (disque réseau ou amovible) reste
  dans la barre latérale, marqué « unavailable », jusqu'à son retrait explicite.
- Chemins dans une réponse : un clic droit sur le corps d'une réponse JSON ou XML copie le
  JSONPath ou le XPath de ce qui est sous le pointeur, seul ou en requête Hurl (`jsonpath "…"`,
  `xpath "string(…)"`), à la manière de « Copy XPath » des outils de développement d'un navigateur
  (`ui/src/paths.ts`). Les XPath suivent les règles de Hurl pour les espaces de noms : préfixes du
  document, `_` pour le premier espace de noms par défaut, `local-name()` pour les autres. Le HTML
  n'est pas proposé : son texte n'est pas lu comme le lit Hurl (éléments non fermés ou implicites).
- Boutons de la fenêtre sous Linux : la barre de titre y est dessinée par GTK, selon le réglage
  `gtk-decoration-layout` du bureau, qui par défaut (GNOME, ou aucune session de bureau comme sous
  WSL) ne contient que le bouton de fermeture. Hurl UI y ajoute les boutons réduire et agrandir
  s'ils manquent (`decoration` dans `src/lib.rs`).
- Instance unique : un second lancement ramène la fenêtre existante au premier plan et se termine
  (`tauri-plugin-single-instance`). Sous Linux, le mécanisme repose sur le bus de session D-Bus :
  sans bus de session, chaque lancement est indépendant, les instances écrivent le même fichier
  d'état et la dernière à enregistrer gagne.
- Les secrets sont stockés en clair dans `secrets.json` du répertoire de configuration de
  l'application, pas dans le trousseau du système. Sous Unix, le fichier est créé avec les droits
  `0600` (il n'est à aucun moment lisible par d'autres), et le répertoire avec `0700` quand c'est
  l'application qui le crée.
- Non repris de Bruno : scripts, gRPC / WebSocket, import Postman / Insomnia / OpenAPI, génération
  de code (la commande `curl` de chaque requête est affichée dans l'onglet Timeline), gestionnaire
  de cookies, workspaces, intégration Git, glisser-déposer dans l'arborescence.
- La barre de titre est celle du système (Bruno dessine la sienne).
