import { Nav } from '@/components/Nav'
import { Home } from '@/pages/Home'
import { LastNight } from '@/pages/LastNight'
import { Sleep } from '@/pages/Sleep'
import { RouterProvider, useRouter } from '@/router'

function Page() {
  const { route } = useRouter()
  switch (route) {
    case '/':
      return <Home />
    case '/sleep':
      return <Sleep />
    case '/last-night':
      return <LastNight />
  }
}

export default function App() {
  return (
    <RouterProvider>
      <div className="min-h-screen min-w-[1200px]">
        <Nav />
        <Page />
      </div>
    </RouterProvider>
  )
}
