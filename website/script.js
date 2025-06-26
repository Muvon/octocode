// Octocode Website JavaScript
document.addEventListener('DOMContentLoaded', function() {
    // Initialize all components
    initializeHighlighting();
    initializeScrollAnimations();
    initializeGitHubStars();
    initializeSmoothScrolling();
    initializeHeaderScroll();
    initializeTypewriter();

    console.log('Octocode website initialized 🚀');
});

// Initialize syntax highlighting
function initializeHighlighting() {
    if (typeof hljs !== 'undefined') {
        hljs.highlightAll();
        console.log('Syntax highlighting initialized');
    }
}

// Initialize scroll animations
function initializeScrollAnimations() {
    const observerOptions = {
        threshold: 0.1,
        rootMargin: '0px 0px -50px 0px'
    };

    const observer = new IntersectionObserver((entries) => {
        entries.forEach(entry => {
            if (entry.isIntersecting) {
                entry.target.classList.add('visible');
            }
        });
    }, observerOptions);

    // Add scroll animation to cards and sections
    const animatedElements = document.querySelectorAll(`
        .feature-card,
        .use-case-card,
        .advantage-card,
        .step,
        .metric-item,
        .lang-item,
        .stat-item
    `);

    animatedElements.forEach(el => {
        el.classList.add('scroll-animate');
        observer.observe(el);
    });
}

// Fetch and display GitHub stars
async function initializeGitHubStars() {
    const starElements = document.querySelectorAll('#star-count, #github-stars-large');

    try {
        const response = await fetch('https://api.github.com/repos/Muvon/octocode');
        const data = await response.json();
        const stars = data.stargazers_count || 0;

        starElements.forEach(element => {
            if (element.id === 'github-stars-large') {
                element.textContent = `⭐ ${formatNumber(stars)}`;
            } else {
                element.textContent = formatNumber(stars);
            }
        });

        console.log(`GitHub stars loaded: ${stars}`);
    } catch (error) {
        console.warn('Failed to load GitHub stars:', error);
        starElements.forEach(element => {
            if (element.id === 'github-stars-large') {
                element.textContent = '⭐ Star on GitHub';
            } else {
                element.textContent = 'Star';
            }
        });
    }
}

// Format numbers for display
function formatNumber(num) {
    if (num >= 1000) {
        return (num / 1000).toFixed(1) + 'k';
    }
    return num.toString();
}

// Initialize smooth scrolling for anchor links
function initializeSmoothScrolling() {
    document.querySelectorAll('a[href^="#"]').forEach(anchor => {
        anchor.addEventListener('click', function (e) {
            e.preventDefault();
            const target = document.querySelector(this.getAttribute('href'));
            if (target) {
                const headerOffset = 80;
                const elementPosition = target.getBoundingClientRect().top;
                const offsetPosition = elementPosition + window.pageYOffset - headerOffset;

                window.scrollTo({
                    top: offsetPosition,
                    behavior: 'smooth'
                });
            }
        });
    });
}

// Header scroll effect
function initializeHeaderScroll() {
    const header = document.querySelector('.header');
    let lastScrollY = window.scrollY;

    window.addEventListener('scroll', () => {
        const currentScrollY = window.scrollY;

        if (currentScrollY > 100) {
            header.style.background = 'rgba(10, 10, 10, 0.98)';
            header.style.boxShadow = '0 2px 20px rgba(0, 0, 0, 0.3)';
        } else {
            header.style.background = 'rgba(10, 10, 10, 0.95)';
            header.style.boxShadow = 'none';
        }

        lastScrollY = currentScrollY;
    });
}

// Typewriter effect for hero section
function initializeTypewriter() {
    const heroSubtitle = document.querySelector('.hero-subtitle');
    if (!heroSubtitle) return;

    const text = heroSubtitle.textContent;
    heroSubtitle.textContent = '';

    let i = 0;
    const typeSpeed = 50;

    function typeWriter() {
        if (i < text.length) {
            heroSubtitle.textContent += text.charAt(i);
            i++;
            setTimeout(typeWriter, typeSpeed);
        }
    }

    // Start typewriter effect after a delay
    setTimeout(typeWriter, 1000);
}

// Add hover effects to cards
document.addEventListener('mouseover', function(e) {
    if (e.target.closest('.feature-card, .use-case-card, .advantage-card')) {
        const card = e.target.closest('.feature-card, .use-case-card, .advantage-card');
        card.style.transform = 'translateY(-8px) scale(1.02)';
    }
});

document.addEventListener('mouseout', function(e) {
    if (e.target.closest('.feature-card, .use-case-card, .advantage-card')) {
        const card = e.target.closest('.feature-card, .use-case-card, .advantage-card');
        card.style.transform = 'translateY(0) scale(1)';
    }
});

// Add click tracking for analytics (placeholder)
function trackEvent(eventName, properties = {}) {
    console.log(`Track: ${eventName}`, properties);
    // Implement your analytics tracking here
    // Example: gtag('event', eventName, properties);
}

// Track button clicks
document.addEventListener('click', function(e) {
    const button = e.target.closest('.btn');
    if (button) {
        const buttonText = button.textContent.trim();
        const buttonType = button.classList.contains('btn-primary') ? 'primary' :
                          button.classList.contains('btn-secondary') ? 'secondary' : 'outline';

        trackEvent('button_click', {
            button_text: buttonText,
            button_type: buttonType,
            page_section: getPageSection(button)
        });
    }
});

// Track external link clicks
document.addEventListener('click', function(e) {
    const link = e.target.closest('a[href^="http"], a[target="_blank"]');
    if (link) {
        const href = link.getAttribute('href');
        const text = link.textContent.trim();

        trackEvent('external_link_click', {
            url: href,
            link_text: text,
            page_section: getPageSection(link)
        });
    }
});

// Get the section of the page where an element is located
function getPageSection(element) {
    const sections = ['hero', 'features', 'mcp-server', 'use-cases', 'tech-stack', 'open-source', 'installation', 'community'];

    for (const section of sections) {
        const sectionElement = document.querySelector(`.${section}`);
        if (sectionElement && sectionElement.contains(element)) {
            return section;
        }
    }

    return 'unknown';
}

// Add loading states for async operations
function showLoading(element) {
    element.classList.add('loading');
}

function hideLoading(element) {
    element.classList.remove('loading');
}

// Utility function to debounce events
function debounce(func, wait) {
    let timeout;
    return function executedFunction(...args) {
        const later = () => {
            clearTimeout(timeout);
            func(...args);
        };
        clearTimeout(timeout);
        timeout = setTimeout(later, wait);
    };
}

// Optimized scroll handler
const optimizedScrollHandler = debounce(() => {
    // Add any scroll-based functionality here
}, 100);

window.addEventListener('scroll', optimizedScrollHandler);

// Add keyboard navigation support
document.addEventListener('keydown', function(e) {
    // Escape key closes any open modals or dropdowns
    if (e.key === 'Escape') {
        // Close any open elements
        document.querySelectorAll('.dropdown-open, .modal-open').forEach(el => {
            el.classList.remove('dropdown-open', 'modal-open');
        });
    }
});

// Add copy to clipboard functionality for code blocks
document.querySelectorAll('.code-block').forEach(codeBlock => {
    const copyButton = document.createElement('button');
    copyButton.textContent = 'Copy';
    copyButton.className = 'copy-button';
    copyButton.style.cssText = `
        position: absolute;
        top: 8px;
        right: 8px;
        background: var(--primary-green);
        color: white;
        border: none;
        padding: 4px 8px;
        border-radius: 4px;
        font-size: 12px;
        cursor: pointer;
        opacity: 0;
        transition: opacity 0.2s;
    `;

    codeBlock.style.position = 'relative';
    codeBlock.appendChild(copyButton);

    codeBlock.addEventListener('mouseenter', () => {
        copyButton.style.opacity = '1';
    });

    codeBlock.addEventListener('mouseleave', () => {
        copyButton.style.opacity = '0';
    });

    copyButton.addEventListener('click', async () => {
        const code = codeBlock.querySelector('code') || codeBlock;
        const text = code.textContent;

        try {
            await navigator.clipboard.writeText(text);
            copyButton.textContent = 'Copied!';
            setTimeout(() => {
                copyButton.textContent = 'Copy';
            }, 2000);

            trackEvent('code_copied', {
                code_snippet: text.substring(0, 50) + '...'
            });
        } catch (err) {
            console.warn('Failed to copy code:', err);
            copyButton.textContent = 'Failed';
            setTimeout(() => {
                copyButton.textContent = 'Copy';
            }, 2000);
        }
    });
});

// Add progressive enhancement for features
if ('IntersectionObserver' in window) {
    // Lazy load images when they come into view
    const imageObserver = new IntersectionObserver((entries) => {
        entries.forEach(entry => {
            if (entry.isIntersecting) {
                const img = entry.target;
                if (img.dataset.src) {
                    img.src = img.dataset.src;
                    img.removeAttribute('data-src');
                    imageObserver.unobserve(img);
                }
            }
        });
    });

    document.querySelectorAll('img[data-src]').forEach(img => {
        imageObserver.observe(img);
    });
}

// Add error handling for failed resources
window.addEventListener('error', function(e) {
    if (e.target.tagName === 'IMG') {
        console.warn('Failed to load image:', e.target.src);
        // Could add fallback image here
    }
});

// Performance monitoring
if ('performance' in window) {
    window.addEventListener('load', () => {
        setTimeout(() => {
            const perfData = performance.getEntriesByType('navigation')[0];
            const loadTime = perfData.loadEventEnd - perfData.loadEventStart;

            console.log(`Page load time: ${loadTime}ms`);

            trackEvent('page_performance', {
                load_time: loadTime,
                dom_content_loaded: perfData.domContentLoadedEventEnd - perfData.domContentLoadedEventStart
            });
        }, 0);
    });
}

// Add service worker registration for offline support (optional)
if ('serviceWorker' in navigator) {
    window.addEventListener('load', () => {
        // Uncomment to enable service worker
        // navigator.serviceWorker.register('/sw.js')
        //     .then(registration => {
        //         console.log('SW registered: ', registration);
        //     })
        //     .catch(registrationError => {
        //         console.log('SW registration failed: ', registrationError);
        //     });
    });
}

// Export functions for potential external use
window.OctocodeWebsite = {
    trackEvent,
    showLoading,
    hideLoading,
    formatNumber
};
