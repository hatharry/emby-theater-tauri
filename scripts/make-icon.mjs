// Downloads the official Emby logo icon (512x512 RGBA, transparent background)
// and upscales it to 1024x1024 at app-icon.png in the repo root, for
// `tauri icon` to consume.
import { execSync } from 'node:child_process';
import { existsSync } from 'node:fs';

if (!existsSync('app-icon.png')) {
  execSync(
    `curl -sfL -o app-icon.png https://raw.githubusercontent.com/MediaBrowser/Emby.Resources/refs/heads/master/images/Logos/logoicon512.png`,
    { stdio: 'inherit' }
  );
  execSync(
    `python3 -c "from PIL import Image; im=Image.open('app-icon.png').convert('RGBA'); im.resize((1024,1024), Image.LANCZOS).save('app-icon.png')"`,
    { stdio: 'inherit' }
  );
  console.log('Downloaded official Emby icon (transparent) -> app-icon.png');
} else {
  console.log('app-icon.png already exists');
}
