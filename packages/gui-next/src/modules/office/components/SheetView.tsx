import { useEffect, useMemo, useState } from 'react'
import { Box, Stack, Typography, ButtonBase, InputBase, Tooltip, alpha, useTheme } from '../../../ui'
import * as XLSX from 'xlsx'

const MAX_ROWS = 1000
const MAX_COLS = 60

/**
 * A spreadsheet as a grid, with a tab per sheet. When `onEditCell` is given,
 * double-clicking a cell edits it in place in the real file.
 */
export function SheetView({ bytes, onEditCell }: { bytes: Uint8Array; onEditCell?: (sheet: string, ref: string, value: string) => Promise<void> }) {
  const theme = useTheme()
  // `sheetStubs` keeps formula cells that have no cached result; without it
  // they are dropped entirely and the grid shows holes where totals are.
  const wb = useMemo(() => XLSX.read(bytes, { type: 'array', cellFormula: true, cellDates: true, sheetStubs: true }), [bytes])
  const [sheet, setSheet] = useState(wb.SheetNames[0] ?? '')
  const [editing, setEditing] = useState<string | null>(null)
  const [draft, setDraft] = useState('')
  const [saving, setSaving] = useState(false)
  useEffect(() => {
    if (!wb.SheetNames.includes(sheet)) setSheet(wb.SheetNames[0] ?? '')
  }, [wb, sheet])

  const ws = wb.Sheets[sheet]
  const range = ws?.['!ref'] ? XLSX.utils.decode_range(ws['!ref']) : null
  const rows = range ? Math.min(range.e.r + 1, MAX_ROWS) : 0
  const cols = range ? Math.min(range.e.c + 1, MAX_COLS) : 0
  const clipped = range && (range.e.r + 1 > MAX_ROWS || range.e.c + 1 > MAX_COLS)

  const commit = async (ref: string) => {
    if (!onEditCell) return
    setSaving(true)
    try {
      await onEditCell(sheet, ref, draft)
      setEditing(null)
    } finally {
      setSaving(false)
    }
  }

  const border = `1px solid ${theme.palette.divider}`
  return (
    <Stack sx={{ flex: 1, minHeight: 0 }}>
      <Box sx={{ flex: 1, overflow: 'auto' }}>
        {!ws || !range ? (
          <Typography variant="caption" color="text.secondary" sx={{ display: 'block', p: 2 }}>
            This sheet is empty.
          </Typography>
        ) : (
          <Box component="table" sx={{ borderCollapse: 'collapse', fontSize: '0.8125rem', fontVariantNumeric: 'tabular-nums' }}>
            <thead>
              <tr>
                <Box component="th" sx={{ position: 'sticky', top: 0, left: 0, zIndex: 3, bgcolor: 'background.paper', border, minWidth: 40 }} />
                {Array.from({ length: cols }, (_, c) => (
                  <Box
                    component="th"
                    key={c}
                    sx={{ position: 'sticky', top: 0, zIndex: 2, bgcolor: 'background.paper', border, px: 1, py: 0.25, fontWeight: 600, color: 'text.secondary', fontSize: '0.75rem' }}
                  >
                    {XLSX.utils.encode_col(c)}
                  </Box>
                ))}
              </tr>
            </thead>
            <tbody>
              {Array.from({ length: rows }, (_, r) => (
                <tr key={r}>
                  <Box
                    component="th"
                    sx={{ position: 'sticky', left: 0, zIndex: 1, bgcolor: 'background.paper', border, px: 1, fontWeight: 600, color: 'text.secondary', fontSize: '0.75rem', textAlign: 'right' }}
                  >
                    {r + 1}
                  </Box>
                  {Array.from({ length: cols }, (_, c) => {
                    const ref = XLSX.utils.encode_cell({ r, c })
                    const cell = ws[ref] as XLSX.CellObject | undefined
                    // A formula with no cached result (the file was written by a
                    // program that doesn't calculate, not by Excel) shows the
                    // formula itself rather than looking like an empty cell.
                    const uncomputed = !!cell?.f && (cell.t === 'z' || cell.v == null || cell.v === '')
                    const shown = !cell ? '' : uncomputed ? `=${cell.f}` : (cell.w ?? String(cell.v ?? ''))
                    const isEditing = editing === ref
                    const numeric = cell?.t === 'n'
                    return (
                      <Tooltip key={c} title={cell?.f ? `=${cell.f}` : ''} placement="top" enterDelay={600} disableInteractive>
                        <Box
                          component="td"
                          onDoubleClick={() => {
                            if (!onEditCell) return
                            setEditing(ref)
                            setDraft(cell?.v == null ? '' : String(cell.v))
                          }}
                          sx={{
                            border,
                            px: 1,
                            py: 0.25,
                            minWidth: 72,
                            maxWidth: 280,
                            whiteSpace: 'nowrap',
                            overflow: 'hidden',
                            textOverflow: 'ellipsis',
                            textAlign: numeric ? 'right' : 'left',
                            cursor: onEditCell ? 'cell' : 'default',
                            bgcolor: isEditing ? alpha(theme.palette.primary.main, 0.1) : cell?.f ? alpha(theme.palette.primary.main, 0.035) : 'transparent',
                            outline: isEditing ? `2px solid ${theme.palette.primary.main}` : 'none',
                            outlineOffset: -1,
                            ...(uncomputed && { color: 'text.disabled', fontStyle: 'italic' }),
                          }}
                        >
                          {isEditing ? (
                            <InputBase
                              autoFocus
                              value={draft}
                              disabled={saving}
                              onChange={(e) => setDraft(e.target.value)}
                              onBlur={() => setEditing(null)}
                              onKeyDown={(e) => {
                                if (e.key === 'Enter') {
                                  e.preventDefault()
                                  void commit(ref)
                                }
                                if (e.key === 'Escape') setEditing(null)
                              }}
                              sx={{ fontSize: '0.8125rem', p: 0, width: '100%', '& input': { p: 0 } }}
                            />
                          ) : (
                            shown
                          )}
                        </Box>
                      </Tooltip>
                    )
                  })}
                </tr>
              ))}
            </tbody>
          </Box>
        )}
      </Box>
      <Stack direction="row" sx={{ alignItems: 'center', gap: 0.5, px: 1, py: 0.5, borderTop: 1, borderColor: 'divider', overflowX: 'auto', flexShrink: 0 }}>
        {wb.SheetNames.map((name) => (
          <ButtonBase
            key={name}
            onClick={() => setSheet(name)}
            sx={{
              px: 1.25,
              py: 0.375,
              borderRadius: '6px',
              fontSize: '0.75rem',
              fontWeight: name === sheet ? 600 : 400,
              color: name === sheet ? 'text.primary' : 'text.secondary',
              bgcolor: name === sheet ? 'action.selected' : 'transparent',
              whiteSpace: 'nowrap',
            }}
          >
            {name}
          </ButtonBase>
        ))}
        <Box sx={{ flex: 1 }} />
        <Typography variant="caption" color="text.disabled" sx={{ whiteSpace: 'nowrap', fontSize: '0.6875rem' }}>
          {clipped ? `Showing the first ${MAX_ROWS} rows × ${MAX_COLS} columns · ` : ''}
          {onEditCell ? 'Double-click a cell to edit' : 'Read-only'}
        </Typography>
      </Stack>
    </Stack>
  )
}
