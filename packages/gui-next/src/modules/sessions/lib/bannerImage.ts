import { BANNER_H, BANNER_W, fitBanner } from './banner'

/** The backend refuses more than 300 KB; stay under it with room for the base64 prefix. */
const MAX_BYTES = 240 * 1024

/** A picture file as a banner: cropped to 3:1 around its centre, scaled to 960×320, JPEG, small enough to keep. */
export async function bannerFromFile(file: File): Promise<string> {
  const bitmap = await createImageBitmap(file)
  const { sx, sy, sw, sh } = fitBanner(bitmap.width, bitmap.height)
  const canvas = document.createElement('canvas')
  canvas.width = BANNER_W
  canvas.height = BANNER_H
  const ctx = canvas.getContext('2d')
  if (!ctx) throw new Error('This browser cannot draw the banner.')
  ctx.drawImage(bitmap, sx, sy, sw, sh, 0, 0, BANNER_W, BANNER_H)
  bitmap.close()
  for (const quality of [0.86, 0.76, 0.66, 0.54, 0.42]) {
    const url = canvas.toDataURL('image/jpeg', quality)
    // base64 is 4 characters for every 3 bytes.
    if (((url.length - url.indexOf(',') - 1) * 3) / 4 <= MAX_BYTES) return url
  }
  throw new Error('That picture is too detailed to keep as a banner. Try a simpler one.')
}
