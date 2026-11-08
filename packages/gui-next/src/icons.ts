import type { SVGProps } from 'react'
import { createElement } from 'react'
// The glyphs, vendored by scripts/gen-icons.mjs (see there for why).
import { solar } from './iconGlyphs'

/**
 * Every icon this app uses, from Solar (Linear style, 24px grid, CC BY 4.0,
 * by 480 Design). One file, so call sites keep importing stable names
 * (`SettingsLinear`, `CloseLinear`) with the same `{ size, color }` props and
 * the name-to-glyph mapping lives in exactly one place. Solar's strokes are
 * 1.5 on the 24px grid and scale with the icon; colour comes from `currentColor`.
 */
interface IconProps extends Omit<SVGProps<SVGSVGElement>, 'color' | 'ref'> {
  size?: number
  color?: string
  strokeWidth?: number
}

function icon(name: string, style: 'linear' | 'bold' = 'linear') {
  const body = solar[`${name}-${style}`]
  return function WrappedIcon({ size = 20, color = 'currentColor', strokeWidth: _strokeWidth, ...rest }: IconProps) {
    return createElement('svg', {
      width: size,
      height: size,
      viewBox: '0 0 24 24',
      fill: 'none',
      color,
      'aria-hidden': true,
      dangerouslySetInnerHTML: { __html: body },
      ...(rest as object),
    })
  }
}

export const AddCircleLinear = icon('add-circle')
export const AddLinear = icon('add')
export const ArchiveBookLinear = icon('book-2')
export const ArchiveLinear = icon('archive')
export const ArrowDownLinear = icon('alt-arrow-down')
export const ChevronRightLinear = icon('alt-arrow-right')
export const FolderOpenLinear = icon('folder-open')
export const ArrowLeftLinear = icon('arrow-left')
export const ArrowRightLinear = icon('arrow-right')
export const ArrowUpLinear = icon('arrow-up')
export const BookLinear = icon('book')
export const ChartLinear = icon('chart-2')
export const CheckLinear = icon('check-read')
export const ColorSwatchLinear = icon('palette')
export const ClipboardTextLinear = icon('clipboard-text')
export const ClockLinear = icon('clock-circle')
export const CloseCircleLinear = icon('close-circle')
export const CloseLinear = icon('close')
export const CodeLinear = icon('code')
export const CommandLinear = icon('command')
export const CommandSquareLinear = icon('code-square')
export const CopyLinear = icon('copy')
export const CopySuccessLinear = icon('copy-check')
export const CpuLinear = icon('cpu')
export const DangerLinear = icon('danger-triangle')
export const DocumentLinear = icon('file')
export const DocumentTextLinear = icon('file-text')
export const DocumentUploadLinear = icon('file-send')
export const EditLinear = icon('pen-new-square')
export const ImageLinear = icon('gallery')
export const ExportLinear = icon('export')
export const FlashLinear = icon('bolt')
export const FolderAddLinear = icon('add-folder')
export const FolderLinear = icon('folder')
export const GithubLinear = icon('code-circle')
export const GlobalLinear = icon('globe')
export const HomeLinear = icon('home')
export const Hierarchy2Linear = icon('git-branch')
export const ImportLinear = icon('import')
export const KeyLinear = icon('key')
export const LayerLinear = icon('layers')
export const Link2Linear = icon('link')
export const LockLinear = icon('lock')
export const MagicpenLinear = icon('magic-stick')
export const MessageAddLinear = icon('chat-round-add')
export const MessageQuestionLinear = icon('question-circle')
export const MessageSquareLinear = icon('chat-square')
export const MonitorLinear = icon('monitor')
export const WalletLinear = icon('wallet')
export const MailLinear = icon('letter')
export const MessageLinear = icon('chat-round-line')
export const CloudLinear = icon('cloud')
export const CubeLinear = icon('box')
export const PowerLinear = icon('power')
export const SparklesLinear = icon('stars')
export const RepeatLinear = icon('repeat')
export const PaperclipLinear = icon('paperclip')
export const PlayCircleLinear = icon('play-circle')
export const PlayLinear = icon('play')
export const RefreshLinear = icon('refresh')
export const SearchNormalLinear = icon('magnifier')
export const SendLinear = icon('arrow-up')
export const Setting2Linear = icon('settings-minimalistic')
export const SettingLinear = icon('settings-minimalistic')
export const SettingsLinear = icon('settings')
export const ShieldTickLinear = icon('shield-check')
export const SparkleLinear = icon('stars')
export const StopCircleLinear = icon('stop-circle')
export const StopLinear = icon('stop')
export const TaskLinear = icon('checklist')
export const TickCircleLinear = icon('check-circle')
export const TrashLinear = icon('trash-bin-minimalistic')
export const UndoLinear = icon('undo-left')
export const RedoLinear = icon('undo-right')
export const MaximizeLinear = icon('maximize-square')
export const ExpandLinear = icon('maximize')
export const FullScreenLinear = icon('full-screen')
export const ExitFullScreenLinear = icon('quit-full-screen')
export const MinimizeLinear = icon('minimize-square')
export const ContextLinear = icon('database')
export const PanelsLinear = icon('panel-right')
export const LayoutLinear = icon('panels-top-left')
export const SidebarLinear = icon('panel-left')
export const AgentsLinear = icon('users-group-rounded')
export const UserPlusLinear = icon('user-plus')
export const UnlockLinear = icon('lock-unlocked')
export const UserOctagonLinear = icon('user-circle')
export const MicrophoneLinear = icon('microphone')
export const StarLinear = icon('star')
export const ZoomInLinear = icon('magnifier-zoom-in')
export const ZoomOutLinear = icon('magnifier-zoom-out')
export const ImageBrokenLinear = icon('gallery-remove')
export const DownloadLinear = icon('download-minimalistic')
export const ComputerLinear = icon('monitor')
export const BrainLinear = icon('brain')
export const VolumeLinear = icon('volume-loud')
export const VolumeOffLinear = icon('volume-cross')
export const TextWrapLinear = icon('text-field')
export const ViewLinear = icon('eye')
export const ViewOffLinear = icon('eye-closed')
export const LayoutTwoColumnLinear = icon('panel-right')
export const FileViewLinear = icon('file-search')
export const UnfoldLessLinear = icon('double-alt-arrow-up')
export const UnfoldMoreLinear = icon('double-alt-arrow-down')
export const MoreHorizontalLinear = icon('menu-dots')

/** The star, filled: a favourite. */
export const StarBold = icon('star', 'bold')
