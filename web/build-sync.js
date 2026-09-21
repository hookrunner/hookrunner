// The page and game package have one identity. Never run a superseded package.
function watchBuild({ build, onCurrent, onReload, onError }) {
  let started = false;
  let reloading = false;
  let pending;

  async function probe() {
    if (reloading) return;
    try {
      const response = await fetch('./build.json', {
        cache: 'no-store',
        headers: { 'X-Hookrunner-Build': build },
      });
      if (!response.ok) throw new Error('The current game build is unavailable. Retrying…');
      const latest = (await response.json()).build;
      if (!/^[a-f0-9]{16}$/.test(latest)) throw new Error('Invalid game build metadata.');
      if (latest === build) {
        if (!started) {
          started = true;
          Promise.resolve().then(onCurrent).catch(onError);
        }
        return;
      }

      reloading = true;
      onReload();
      // The mismatch response clears HTTP cache via Clear-Site-Data: "cache".
      // Also explicitly fetch the entry page from the network before reloading
      // the SAME address. Check it matches so a partial publish cannot loop.
      const fresh = await fetch(location.href, { cache: 'reload' });
      const page = fresh.ok ? await fresh.text() : '';
      if (!page.includes(`data-build="${latest}"`)) {
        reloading = false;
        return;
      }
      location.reload();
    } catch (error) {
      reloading = false;
      if (!started) onError(error);
    }
  }

  function check() {
    pending ??= probe().finally(() => { pending = undefined; });
    return pending;
  }
  const visible = () => { if (document.visibilityState === 'visible') void check(); };
  const resume = () => { void check(); };
  setInterval(resume, 2000);
  addEventListener('focus', resume);
  addEventListener('pageshow', resume);
  document.addEventListener('visibilitychange', visible);
  void check();
}
