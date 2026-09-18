// Downloads the official Emby logo icon (512x512 RGBA, transparent background)
// and upscales it to 1024x1024 at src/app-icon.png, for `tauri icon` to consume.
import { execSync } from 'node:child_process';
import { existsSync } from 'node:fs';

const ICON = 'src/app-icon.png';

if (!existsSync(ICON)) {
  execSync(
    `curl -sfL -o ${ICON} https://raw.githubusercontent.com/MediaBrowser/Emby.Resources/refs/heads/master/images/Logos/logoicon512.png`,
    { stdio: 'inherit' }
  );
  execSync(
    `python3 -c "from PIL import Image; im=Image.open('${ICON}').convert('RGBA'); im.resize((1024,1024), Image.LANCZOS).save('${ICON}')"`,
    { stdio: 'inherit' }
  );
  console.log('Downloaded official Emby icon (transparent) -> src/app-icon.png');
} else {
  console.log('src/app-icon.png already exists');
}
