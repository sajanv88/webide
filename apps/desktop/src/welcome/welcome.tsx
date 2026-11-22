import "./welcome.scss";
import { HexagonBackground } from "@nuvui/backgrounds/hexagon-background";
import { FolderIcon, FolderOpenIcon, PauseIcon, PlayIcon } from "@nuvui/icons";
import { Button } from "@nuvui/react";
import type { AppCommandId, RecentWorkspaceDto } from "@repo/contracts";
import type { CommandRegistry } from "@repo/kernel";
import { useId, useState } from "react";

interface WelcomeProps {
  readonly recent: readonly RecentWorkspaceDto[];
  readonly registry: CommandRegistry<AppCommandId>;
}

/**
 * The window before any folder is open, after nuvui's empty-projects block:
 * what to do, the way to do it, and the folders opened before, over hexagons
 * that light up under the pointer.
 */
export function Welcome({ recent, registry }: WelcomeProps) {
  // Anything that moves for more than five seconds needs a way to stop it.
  const [paused, setPaused] = useState(false);
  const recentHeading = useId();

  return (
    <div className="welcome">
      <HexagonBackground className="welcome__stage" scrim="strong" paused={paused}>
        <div className="welcome__inner">
          <span className="welcome__icon">
            <FolderOpenIcon size={24} />
          </span>
          <h2 className="welcome__heading">No folder open</h2>
          <p className="welcome__text">
            Open a folder to browse its files. Everything you do stays inside the folder you choose.
          </p>
          <div className="welcome__actions">
            <Button onClick={() => void registry.execute("workspace.openFolder")}>
              <FolderOpenIcon size={16} />
              Open folder…
            </Button>
          </div>
          {recent.length > 0 ? (
            <section className="welcome__recent" aria-labelledby={recentHeading}>
              <h3 id={recentHeading} className="welcome__recent-title">
                Recent
              </h3>
              <ul className="welcome__list">
                {recent.map((entry) => (
                  <li key={entry.path}>
                    <Button
                      intent="ghost"
                      className="welcome__entry"
                      onClick={() =>
                        void registry.execute("workspace.openRecent", { path: entry.path })
                      }
                    >
                      <FolderIcon size={16} className="welcome__entry-icon" />
                      <span className="welcome__entry-name">{entry.name}</span>
                      <span className="welcome__entry-path">{entry.path}</span>
                    </Button>
                  </li>
                ))}
              </ul>
            </section>
          ) : null}
        </div>
        <Button
          intent="ghost"
          size="sm"
          className="welcome__pause"
          aria-pressed={paused}
          onClick={() => setPaused((now) => !now)}
        >
          {paused ? <PlayIcon size={16} /> : <PauseIcon size={16} />}
          Pause the background
        </Button>
      </HexagonBackground>
    </div>
  );
}
