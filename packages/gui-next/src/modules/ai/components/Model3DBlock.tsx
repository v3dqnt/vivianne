import { useEffect, useRef, useState } from 'react'
import { Box, Stack, Typography, CircularProgress, useTheme } from '../../../ui'
// Types only — erased at build, so three.js itself still loads lazily below.
import type * as THREE_T from 'three'

/** A declarative scene of primitives, or a model file to load. */
interface Primitive {
  shape: 'box' | 'sphere' | 'cylinder' | 'cone' | 'torus' | 'plane'
  /** Shape-specific size; sensible defaults per shape when omitted. */
  size?: number[]
  position?: [number, number, number]
  rotation?: [number, number, number]
  color?: string
  opacity?: number
  wireframe?: boolean
}
export interface SceneSpec {
  title?: string
  /** Load a model from disk instead of building primitives. */
  src?: string
  objects?: Primitive[]
  background?: string
  /** Slow auto-rotation. Off by default: motion should be motivated. */
  autoRotate?: boolean
  grid?: boolean
}

const HEIGHT = 320

/**
 * Renders a ```model3d fenced block as an interactive three.js scene.
 *
 * Two modes, chosen by whether `src` is present:
 *   - a declarative list of primitives (a quick diagram of a mechanism, a
 *     layout sketch, a geometry the agent computed), and
 *   - a model file from disk (`.glb`/`.gltf`), which is what makes this useful
 *     against a real project rather than a toy.
 *
 * three.js is imported dynamically so its ~600KB only loads for a message that
 * actually contains a 3D block — the chat should not pay for it otherwise.
 */
export function Model3DBlock({ code }: { code: string }) {
  const theme = useTheme()
  const mountRef = useRef<HTMLDivElement | null>(null)
  const [error, setError] = useState<string | null>(null)
  const [loading, setLoading] = useState(true)
  const [spec, setSpec] = useState<SceneSpec | null>(null)

  useEffect(() => {
    let parsed: SceneSpec
    try {
      parsed = JSON.parse(code)
    } catch (e) {
      setError(`Not valid JSON: ${e instanceof Error ? e.message : String(e)}`)
      setLoading(false)
      return
    }
    if (!parsed.src && (!Array.isArray(parsed.objects) || parsed.objects.length === 0)) {
      setError('Provide "src" (a .glb/.gltf path) or "objects": [{shape, ...}, ...].')
      setLoading(false)
      return
    }
    setSpec(parsed)
    setError(null)
  }, [code])

  useEffect(() => {
    if (!spec || !mountRef.current) return
    const mount = mountRef.current
    let disposed = false
    // Held so the cleanup can tear the whole scene down deterministically —
    // a WebGL context leaked per message would exhaust the browser's limit
    // (typically ~16) after a handful of 3D blocks in one conversation.
    let cleanup: (() => void) | undefined

    void (async () => {
      try {
        const THREE = await import('three')
        const { OrbitControls } = await import('three/examples/jsm/controls/OrbitControls.js')
        if (disposed) return

        const width = mount.clientWidth || 640
        const scene = new THREE.Scene()
        if (spec.background) scene.background = new THREE.Color(spec.background)

        const camera = new THREE.PerspectiveCamera(50, width / HEIGHT, 0.1, 1000)
        camera.position.set(4, 3, 6)

        const renderer = new THREE.WebGLRenderer({ antialias: true, alpha: !spec.background })
        renderer.setPixelRatio(Math.min(window.devicePixelRatio, 2))
        renderer.setSize(width, HEIGHT)
        mount.appendChild(renderer.domElement)

        const controls = new OrbitControls(camera, renderer.domElement)
        controls.enableDamping = true
        // Respect the user's motion preference for the idle spin only; manual
        // orbiting is a direct response to input and stays available.
        const reduceMotion = window.matchMedia?.('(prefers-reduced-motion: reduce)').matches
        controls.autoRotate = !!spec.autoRotate && !reduceMotion
        controls.autoRotateSpeed = 1.2

        scene.add(new THREE.AmbientLight(0xffffff, 0.65))
        const key = new THREE.DirectionalLight(0xffffff, 1.6)
        key.position.set(5, 8, 6)
        scene.add(key)

        if (spec.grid !== false) {
          const grid = new THREE.GridHelper(20, 20, theme.palette.divider, theme.palette.divider)
          ;(grid.material as THREE_T.Material).opacity = 0.25
          ;(grid.material as THREE_T.Material).transparent = true
          scene.add(grid)
        }

        if (spec.src) {
          const { GLTFLoader } = await import('three/examples/jsm/loaders/GLTFLoader.js')
          if (disposed) return
          // Local files reach the webview through Tauri's asset protocol; a
          // plain filesystem path is not fetchable from the renderer.
          let url = spec.src
          if (!/^https?:|^blob:|^data:/.test(url)) {
            try {
              const { convertFileSrc } = await import('@tauri-apps/api/core')
              url = convertFileSrc(spec.src)
            } catch {
              // Non-Tauri context (browser dev): fall through to the raw path.
            }
          }
          await new Promise<void>((resolve, reject) => {
            new GLTFLoader().load(
              url,
              (gltf) => {
                // Frame the model regardless of its authored scale, so a
                // millimetre-unit CAD export and a metre-unit game asset both
                // arrive filling the view.
                const box = new THREE.Box3().setFromObject(gltf.scene)
                const size = box.getSize(new THREE.Vector3())
                const center = box.getCenter(new THREE.Vector3())
                const maxDim = Math.max(size.x, size.y, size.z) || 1
                gltf.scene.scale.setScalar(3 / maxDim)
                gltf.scene.position.sub(center.multiplyScalar(3 / maxDim))
                scene.add(gltf.scene)
                resolve()
              },
              undefined,
              (e) => reject(e),
            )
          })
        } else {
          for (const o of spec.objects ?? []) {
            const s = o.size ?? []
            let geo: THREE_T.BufferGeometry
            switch (o.shape) {
              case 'sphere':
                geo = new THREE.SphereGeometry(s[0] ?? 1, 32, 24)
                break
              case 'cylinder':
                geo = new THREE.CylinderGeometry(s[0] ?? 1, s[1] ?? s[0] ?? 1, s[2] ?? 2, 32)
                break
              case 'cone':
                geo = new THREE.ConeGeometry(s[0] ?? 1, s[1] ?? 2, 32)
                break
              case 'torus':
                geo = new THREE.TorusGeometry(s[0] ?? 1, s[1] ?? 0.4, 16, 48)
                break
              case 'plane':
                geo = new THREE.PlaneGeometry(s[0] ?? 2, s[1] ?? 2)
                break
              default:
                geo = new THREE.BoxGeometry(s[0] ?? 1, s[1] ?? s[0] ?? 1, s[2] ?? s[0] ?? 1)
            }
            const mat = new THREE.MeshStandardMaterial({
              color: o.color ?? theme.palette.primary.main,
              transparent: (o.opacity ?? 1) < 1,
              opacity: o.opacity ?? 1,
              wireframe: !!o.wireframe,
              roughness: 0.45,
              metalness: 0.1,
            })
            const mesh = new THREE.Mesh(geo, mat)
            if (o.position) mesh.position.set(...o.position)
            if (o.rotation) mesh.rotation.set(...o.rotation)
            scene.add(mesh)
          }
        }

        let frame = 0
        const tick = () => {
          frame = requestAnimationFrame(tick)
          controls.update()
          renderer.render(scene, camera)
        }
        tick()

        const onResize = () => {
          const w = mount.clientWidth || width
          camera.aspect = w / HEIGHT
          camera.updateProjectionMatrix()
          renderer.setSize(w, HEIGHT)
        }
        const observer = new ResizeObserver(onResize)
        observer.observe(mount)

        cleanup = () => {
          cancelAnimationFrame(frame)
          observer.disconnect()
          controls.dispose()
          scene.traverse((obj) => {
            const m = obj as THREE_T.Mesh
            m.geometry?.dispose?.()
            const mat = m.material as THREE_T.Material | THREE_T.Material[] | undefined
            if (Array.isArray(mat)) mat.forEach((x) => x.dispose())
            else mat?.dispose?.()
          })
          renderer.dispose()
          // Releases the WebGL context immediately rather than at GC.
          renderer.forceContextLoss?.()
          if (renderer.domElement.parentNode === mount) mount.removeChild(renderer.domElement)
        }
        setLoading(false)
      } catch (e) {
        if (!disposed) {
          setError(e instanceof Error ? e.message : String(e))
          setLoading(false)
        }
      }
    })()

    return () => {
      disposed = true
      cleanup?.()
    }
  }, [spec, theme.palette.divider, theme.palette.primary.main])

  if (error) {
    return (
      <Box sx={{ my: 1, p: 1.5, border: 1, borderColor: 'divider', borderRadius: '12px' }}>
        <Typography variant="caption" color="error.main" sx={{ fontWeight: 600, display: 'block' }}>
          3D scene problem
        </Typography>
        <Typography variant="caption" color="text.secondary">
          {error}
        </Typography>
      </Box>
    )
  }

  return (
    <Box sx={{ my: 1, border: 1, borderColor: 'divider', borderRadius: '12px', overflow: 'hidden', bgcolor: 'background.paper' }}>
      {spec?.title && (
        <Stack direction="row" sx={{ alignItems: 'center', px: 1.5, py: 0.75, borderBottom: 1, borderColor: 'divider' }}>
          <Typography variant="body2" sx={{ fontWeight: 600 }}>
            {spec.title}
          </Typography>
          <Typography variant="caption" color="text.secondary" sx={{ ml: 'auto' }}>
            drag to orbit · scroll to zoom
          </Typography>
        </Stack>
      )}
      <Box sx={{ position: 'relative', height: HEIGHT }}>
        <Box ref={mountRef} sx={{ width: '100%', height: HEIGHT }} />
        {loading && (
          <Stack sx={{ position: 'absolute', inset: 0, alignItems: 'center', justifyContent: 'center', gap: 1 }}>
            <CircularProgress size={18} />
            <Typography variant="caption" color="text.secondary">
              Loading scene…
            </Typography>
          </Stack>
        )}
      </Box>
    </Box>
  )
}
