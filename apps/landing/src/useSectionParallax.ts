import { useEffect } from "react"

const parallaxSections = "[data-parallax-section]"

function clamp(value: number, limit: number) {
  return Math.max(-limit, Math.min(limit, value))
}

function writeOffsets(
  element: HTMLElement,
  distanceFromCenter: number,
  viewportHeight: number,
  intensity: number,
) {
  const distance = distanceFromCenter * intensity
  const micro = clamp(distance * 0.035, 20 * intensity)
  const slow = clamp(distance * 0.08, 56 * intensity)
  const medium = clamp(distance * 0.22, 160 * intensity)
  const fast = clamp(distance * 0.42, 300 * intensity)
  const normalizedDistance = clamp(distanceFromCenter / Math.max(viewportHeight, 1), 1)
  const scale = 1 + normalizedDistance * 0.03 * intensity
  const scaleReverse = 1 - normalizedDistance * 0.04 * intensity
  const orbitScale = 1 + normalizedDistance * 0.2 * intensity

  element.style.setProperty("--parallax-micro", `${micro.toFixed(2)}px`)
  element.style.setProperty("--parallax-slow", `${slow.toFixed(2)}px`)
  element.style.setProperty("--parallax-medium", `${medium.toFixed(2)}px`)
  element.style.setProperty("--parallax-fast", `${fast.toFixed(2)}px`)
  element.style.setProperty("--parallax-slow-reverse", `${(-slow).toFixed(2)}px`)
  element.style.setProperty("--parallax-medium-reverse", `${(-medium).toFixed(2)}px`)
  element.style.setProperty("--parallax-scale", scale.toFixed(4))
  element.style.setProperty("--parallax-scale-reverse", scaleReverse.toFixed(4))
  element.style.setProperty("--parallax-orbit-scale", orbitScale.toFixed(4))
}

export function useSectionParallax() {
  useEffect(() => {
    if (CSS.supports("animation-timeline", "view()")) {
      return
    }

    const sections = Array.from(document.querySelectorAll<HTMLElement>(parallaxSections))
    const reducedMotion = window.matchMedia("(prefers-reduced-motion: reduce)")
    let frame: number | null = null

    const update = () => {
      frame = null

      const viewportHeight = window.innerHeight

      if (reducedMotion.matches) {
        sections.forEach((section) => writeOffsets(section, 0, viewportHeight, 0))
        return
      }

      const intensity = window.innerWidth <= 660 ? 0.52 : window.innerWidth <= 900 ? 0.75 : 1

      sections.forEach((section) => {
        const bounds = section.getBoundingClientRect()
        const sectionCenter = bounds.top + bounds.height / 2
        const distanceFromCenter = viewportHeight / 2 - sectionCenter

        writeOffsets(section, distanceFromCenter, viewportHeight, intensity)

        if (section.classList.contains("hero")) {
          const exitProgress = Math.max(0, Math.min(1, -bounds.top / Math.max(bounds.height, 1)))
          const motionProgress =
            exitProgress <= 0.7
              ? (exitProgress / 0.7) * 0.88
              : 0.88 + ((exitProgress - 0.7) / 0.3) * 0.12
          const heroArtOffset = (-20 + motionProgress * 300) * intensity
          section.style.setProperty("--parallax-hero-art", `${heroArtOffset.toFixed(2)}px`)
        }
      })
    }

    const queueUpdate = () => {
      if (frame === null) {
        frame = window.requestAnimationFrame(update)
      }
    }

    queueUpdate()
    window.addEventListener("scroll", queueUpdate, { passive: true })
    window.addEventListener("resize", queueUpdate)
    reducedMotion.addEventListener("change", queueUpdate)

    return () => {
      if (frame !== null) {
        window.cancelAnimationFrame(frame)
      }
      window.removeEventListener("scroll", queueUpdate)
      window.removeEventListener("resize", queueUpdate)
      reducedMotion.removeEventListener("change", queueUpdate)
    }
  }, [])
}
