(function () {
  'use strict';

  const navLinks = Array.from(document.querySelectorAll('.nav-links a[href^="#"]'));
  const sections = Array.from(document.querySelectorAll('header[id], section[id]'));

  if ('IntersectionObserver' in window && navLinks.length > 0 && sections.length > 0) {
    const observer = new IntersectionObserver((entries) => {
      entries.forEach((entry) => {
        if (!entry.isIntersecting) return;
        const id = entry.target.id;
        navLinks.forEach((link) => {
          link.classList.toggle('active', link.getAttribute('href') === '#' + id);
        });
      });
    }, { rootMargin: '-35% 0px -55% 0px' });

    sections.forEach((section) => observer.observe(section));
  }

  document.querySelectorAll('[data-copy]').forEach((button) => {
    button.addEventListener('click', async () => {
      const value = button.getAttribute('data-copy');
      if (!value) return;

      try {
        await navigator.clipboard.writeText(value);
        button.classList.add('copied');
        window.setTimeout(() => button.classList.remove('copied'), 1200);
      } catch {
        const code = button.querySelector('code');
        if (code) {
          const selection = window.getSelection();
          const range = document.createRange();
          range.selectNodeContents(code);
          selection.removeAllRanges();
          selection.addRange(range);
        }
      }
    });
  });
}());
