import { createContext, useContext } from "react"

export type Theme = "dark" | "light" | "system"

export interface ThemeContextValue {
  theme: Theme
  setTheme: (theme: Theme) => void
}

export const ThemeContext = createContext<ThemeContextValue>({
  theme: "system",
  setTheme: () => undefined,
})

export function isTheme(value: string): value is Theme {
  return value === "dark" || value === "light" || value === "system"
}

export function useTheme(): ThemeContextValue {
  return useContext(ThemeContext)
}
