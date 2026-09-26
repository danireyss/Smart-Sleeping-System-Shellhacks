// A tiny History-API router: three routes don't need a routing library. The
// backend serves index.html for these paths, so reloads and links work.
import { createContext, useCallback, useContext, useEffect, useState, type ReactNode } from 'react'

export type Route = '/' | '/sleep' | '/last-night'

const ROUTES: Route[] = ['/', '/sleep', '/last-night']
const toRoute = (path: string): Route => (ROUTES.includes(path as Route) ? (path as Route) : '/')

interface Router {
  route: Route
  navigate: (to: Route) => void
}

const RouterContext = createContext<Router | null>(null)

export function RouterProvider({ children }: { children: ReactNode }) {
  const [route, setRoute] = useState<Route>(() => toRoute(window.location.pathname))

  useEffect(() => {
    const onPop = () => setRoute(toRoute(window.location.pathname))
    window.addEventListener('popstate', onPop)
    return () => window.removeEventListener('popstate', onPop)
  }, [])

  const navigate = useCallback((to: Route) => {
    if (to !== window.location.pathname) window.history.pushState(null, '', to)
    setRoute(to)
    window.scrollTo(0, 0)
  }, [])

  return <RouterContext value={{ route, navigate }}>{children}</RouterContext>
}

export function useRouter(): Router {
  const router = useContext(RouterContext)
  if (!router) throw new Error('useRouter outside RouterProvider')
  return router
}
