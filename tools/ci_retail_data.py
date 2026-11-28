"""Download the private retail fixture for a GitHub Actions job and set RA2_DIR."""

import os
from pathlib import Path
import shutil
import tempfile
import urllib.error
import urllib.request
import zipfile


def main():
    url = os.environ.get('RETAIL_GAME_DATA_URL')
    if not url:
        raise SystemExit('Set the RETAIL_GAME_DATA_URL secret to the retail fixture ZIP.')
    retail = Path(tempfile.mkdtemp(prefix='retail-', dir=os.environ['RUNNER_TEMP']))
    # Cloudflare refuses Python's default user agent. Keep the URL out of logs.
    with tempfile.TemporaryFile(dir=retail) as download:
        try:
            request = urllib.request.Request(url, headers={'User-Agent': 'vera20k-ci'})
            with urllib.request.urlopen(request) as response:
                shutil.copyfileobj(response, download)
        except urllib.error.HTTPError as error:
            raise SystemExit(f'Retail fixture download failed (HTTP {error.code}).') from None
        except (OSError, ValueError):
            raise SystemExit('Retail fixture download failed.') from None
        with zipfile.ZipFile(download) as archive:
            archive.extractall(retail)
    master = next((path for path in retail.rglob('*') if path.name.lower() == 'ra2md.mix'), None)
    if master is None:
        raise SystemExit('Retail fixture lacks ra2md.mix.')
    with open(os.environ['GITHUB_ENV'], 'a', encoding='utf-8') as env:
        env.write(f'RA2_DIR={master.parent.resolve()}\n')


if __name__ == '__main__':
    main()
