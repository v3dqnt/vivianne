import type { SVGProps } from 'react'
import { createElement } from 'react'
import { HugeiconsIcon, type IconSvgElement } from '@hugeicons/react'
// The glyphs, vendored by scripts/gen-icons.mjs (see there for why).
import * as G from './iconGlyphs'

/**
 * Every icon this app uses, from Hugeicons (Terax's icon set) — one file so
 * call sites keep importing stable names (`SettingsLinear`, `CloseLinear`)
 * with the same `{ size, color }` props, and the name-to-glyph mapping lives
 * in exactly one place.
 *
 * Terax draws icons at a 1.75 stroke by default (2 for small emphasised
 * glyphs, 1.5 for large ones), muted at rest and ink on hover — the colour
 * comes from `currentColor`, as every call site already passes.
 */
interface IconProps extends Omit<SVGProps<SVGSVGElement>, 'color' | 'ref'> {
  size?: number
  color?: string
  strokeWidth?: number
}

function icon(glyph: IconSvgElement) {
  return function WrappedIcon({ size = 20, color = 'currentColor', strokeWidth, ...rest }: IconProps) {
    // Thin strokes vanish at small sizes and shout at large ones.
    const stroke = strokeWidth ?? (size <= 12 ? 2 : size >= 20 ? 1.5 : 1.75)
    return createElement(HugeiconsIcon, { icon: glyph, size, color, strokeWidth: stroke, ...(rest as object) })
  }
}

export const AddCircleLinear = icon(G.AddCircleIcon)
export const AddLinear = icon(G.Add01Icon)
export const ArchiveBookLinear = icon(G.BookOpen01Icon)
export const ArchiveLinear = icon(G.Archive01Icon)
export const ArrowDownLinear = icon(G.ArrowDown01Icon)
export const ChevronRightLinear = icon(G.ArrowRight01Icon)
export const FolderOpenLinear = icon(G.FolderOpenIcon)
export const ArrowLeftLinear = icon(G.ArrowLeft02Icon)
export const ArrowRightLinear = icon(G.ArrowRight02Icon)
export const ArrowUpLinear = icon(G.ArrowUp02Icon)
export const BookLinear = icon(G.Book02Icon)
export const ChartLinear = icon(G.ChartColumnIcon)
export const CheckLinear = icon(G.Tick02Icon)
export const ColorSwatchLinear = icon(G.PaintBoardIcon)
export const ClipboardTextLinear = icon(G.TaskDaily01Icon)
export const ClockLinear = icon(G.Clock01Icon)
export const CloseCircleLinear = icon(G.CancelCircleIcon)
export const CloseLinear = icon(G.Cancel01Icon)
export const CodeLinear = icon(G.SourceCodeIcon)
export const CommandLinear = icon(G.CommandLineIcon)
export const CommandSquareLinear = icon(G.ComputerTerminal01Icon)
export const CopyLinear = icon(G.Copy01Icon)
export const CopySuccessLinear = icon(G.CopyCheckIcon)
export const CpuLinear = icon(G.CpuIcon)
export const DangerLinear = icon(G.Alert02Icon)
export const DocumentLinear = icon(G.File01Icon)
export const DocumentTextLinear = icon(G.FileEditIcon)
export const DocumentUploadLinear = icon(G.FileUploadIcon)
export const EditLinear = icon(G.PencilEdit02Icon)
export const ExportLinear = icon(G.Upload04Icon)
export const FlashLinear = icon(G.FlashIcon)
export const FolderAddLinear = icon(G.FolderAddIcon)
export const FolderLinear = icon(G.Folder01Icon)
// Hugeicons' free set includes the GitHub mark.
export const GithubLinear = icon(G.GithubIcon)
export const GlobalLinear = icon(G.Globe02Icon)
export const Hierarchy2Linear = icon(G.GitBranchIcon)
export const ImportLinear = icon(G.Download04Icon)
export const KeyLinear = icon(G.Key01Icon)
export const LayerLinear = icon(G.Layers01Icon)
export const Link2Linear = icon(G.Link01Icon)
export const LockLinear = icon(G.LockIcon)
export const MagicpenLinear = icon(G.MagicWand01Icon)
export const MessageAddLinear = icon(G.MessageAdd01Icon)
export const MessageQuestionLinear = icon(G.HelpCircleIcon)
export const MessageSquareLinear = icon(G.Message01Icon)
export const MonitorLinear = icon(G.ComputerIcon)
export const PaperclipLinear = icon(G.Attachment01Icon)
export const PlayCircleLinear = icon(G.PlayCircleIcon)
export const PlayLinear = icon(G.PlayIcon)
export const RefreshLinear = icon(G.RefreshIcon)
export const SearchNormalLinear = icon(G.Search01Icon)
/** Send: an up arrow, like Terax's composer. */
export const SendLinear = icon(G.ArrowUp02Icon)
export const Setting2Linear = icon(G.Settings02Icon)
export const SettingLinear = icon(G.Settings02Icon)
export const SettingsLinear = icon(G.Settings01Icon)
export const ShieldTickLinear = icon(G.SecurityCheckIcon)
export const SparkleLinear = icon(G.SparklesIcon)
export const StopCircleLinear = icon(G.StopCircleIcon)
export const StopLinear = icon(G.StopIcon)
export const TaskLinear = icon(G.TaskDone01Icon)
export const TickCircleLinear = icon(G.CheckmarkCircle02Icon)
export const TrashLinear = icon(G.Delete02Icon)
export const UndoLinear = icon(G.Undo02Icon)
export const RedoLinear = icon(G.Redo02Icon)
export const MaximizeLinear = icon(G.Maximize01Icon)
export const MinimizeLinear = icon(G.Minimize01Icon)
/** Context window — a stack of data filling up. */
export const ContextLinear = icon(G.DatabaseIcon)
/** The Panels menu: which panels are open. */
export const PanelsLinear = icon(G.LayoutTwoColumnIcon)
/** The Layout menu: starting arrangements. Not the branch icon — one
 * glyph meaning two things is worse than none. */
export const LayoutLinear = icon(G.DashboardSquare01Icon)
export const AgentsLinear = icon(G.UserGroupIcon)
export const UserPlusLinear = icon(G.UserAdd01Icon)
export const UnlockLinear = icon(G.SquareUnlock02Icon)
export const UserOctagonLinear = icon(G.UserCircleIcon)
export const MicrophoneLinear = icon(G.Mic01Icon)
