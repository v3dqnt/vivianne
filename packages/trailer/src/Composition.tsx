import { AbsoluteFill, Audio, Composition, Series, staticFile } from "remotion";
import { MagnifyingGlass, Terminal, Brain, FlowArrow } from "@phosphor-icons/react";
import { QuietOpen } from "./scenes/QuietOpen";
import { WordmarkReveal } from "./scenes/WordmarkReveal";
import { FeatureBeat } from "./scenes/FeatureBeat";
import { Closing } from "./scenes/Closing";

import { GuiCodebaseMockup } from "./components/GuiCodebaseMockup";
import { GuiEditCommandMockup } from "./components/GuiEditCommandMockup";
import { GuiMemoryMockup } from "./components/GuiMemoryMockup";
import { GuiAutonomousMockup } from "./components/GuiAutonomousMockup";

export const TRAILER_FPS = 30;
export const TRAILER_DURATION = 660; // 22s @ 30fps — matches the trimmed audio excerpt exactly
export const TRAILER_WIDTH = 2160;
export const TRAILER_HEIGHT = 3840;

const Trailer: React.FC = () => {
  return (
    <AbsoluteFill style={{ background: "#ffffff" }}>
      <Audio src={staticFile("audio/theme.mp3")} />
      <Series>
        {/* Intro beats (0.600s, 1.180s, 1.750s -> Beat 3 finishes at Frame 52) */}
        <Series.Sequence durationInFrames={52}>
          <QuietOpen />
        </Series.Sequence>
        
        {/* Title reveal beats (2.330s -> 5.020s -> Beat 8 finishes at Frame 151) */}
        <Series.Sequence durationInFrames={99}>
          <WordmarkReveal />
        </Series.Sequence>
        
        {/* Feature 1 beats (5.980s -> 8.090s -> Beat 12 finishes at Frame 243) */}
        <Series.Sequence durationInFrames={92}>
          <FeatureBeat
            index="01"
            IconComponent={MagnifyingGlass}
            heading="Reads and understands your codebase"
            description="Real files, real context — not a guess from a snippet."
            MockupComponent={GuiCodebaseMockup}
          />
        </Series.Sequence>
        
        {/* Feature 2 beats (9.240s -> 11.160s -> Beat 16 finishes at Frame 335) */}
        <Series.Sequence durationInFrames={92}>
          <FeatureBeat
            index="02"
            IconComponent={Terminal}
            heading="Writes, edits, and runs commands"
            description="Ships changes end to end, not just suggestions."
            MockupComponent={GuiEditCommandMockup}
          />
        </Series.Sequence>
        
        {/* Feature 3 beats (12.120s -> 13.470s -> Beat 19 finishes at Frame 404? Beat 20 finishes at Frame 427) */}
        <Series.Sequence durationInFrames={92}>
          <FeatureBeat
            index="03"
            IconComponent={Brain}
            heading="Remembers, session to session"
            description="Context that persists — no re-explaining yourself."
            MockupComponent={GuiMemoryMockup}
          />
        </Series.Sequence>
        
        {/* Feature 4 beats (14.240s -> 16.730s -> Beat 23 finishes at Frame 502) */}
        <Series.Sequence durationInFrames={75}>
          <FeatureBeat
            index="04"
            IconComponent={FlowArrow}
            heading="Works autonomously, start to finish"
            description="Give it a task. It sees it through."
            MockupComponent={GuiAutonomousMockup}
          />
        </Series.Sequence>
        
        {/* Unified Outro (starts at 16.730s -> Ends at Frame 660 / Beat 29) */}
        <Series.Sequence durationInFrames={158}>
          <Closing />
        </Series.Sequence>
      </Series>
    </AbsoluteFill>
  );
};

export const MyComposition = () => {
  return (
    <Composition
      id="VivianneTrailer"
      component={Trailer}
      durationInFrames={TRAILER_DURATION}
      fps={TRAILER_FPS}
      width={TRAILER_WIDTH}
      height={TRAILER_HEIGHT}
    />
  );
};
