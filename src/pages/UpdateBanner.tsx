import { type ReactNode, useState } from "react";
import { api } from "../api";
import { Button } from "../components";
import type { UpdateInfo } from "../types";

const BYTES_PER_MB = 1024 * 1024;

function megabytes(bytes: number): string {
  return (bytes / BYTES_PER_MB).toFixed(1);
}

/** The newer release on the Status page: the choices, the download and its outcome. */
export default function UpdateBanner({ update }: { update: UpdateInfo }) {
  const [problem, setProblem] = useState<string | null>(null);

  const run = async (action: () => Promise<void>) => {
    setProblem(null);
    try {
      await action();
    } catch (e) {
      setProblem(String(e));
    }
  };

  const version = <span className="mono">{update.version}</span>;
  let text: ReactNode;
  let buttons: ReactNode = null;

  if (!update.installable) {
    text = (
      <>
        Deskmate {version} is available. Install it over this version; settings and pairing are
        kept.
      </>
    );
    buttons = (
      <Button kind="primary" onClick={() => void run(api.openUpdatePage)}>
        Download
      </Button>
    );
  } else if (update.phase === "downloading") {
    text = (
      <>
        Downloading Deskmate {version}
        {update.total_bytes !== null && (
          <span className="mono text-muted">
            {" "}
            {megabytes(update.downloaded_bytes)} of {megabytes(update.total_bytes)} MB
          </span>
        )}
      </>
    );
  } else if (update.phase === "installing") {
    text = <>Installing Deskmate {version}. Deskmate restarts when it is done.</>;
  } else if (update.phase === "failed") {
    text = (
      <>
        Deskmate {version} was not installed. {update.error}
      </>
    );
    buttons = (
      <>
        <Button kind="primary" onClick={() => void run(api.installUpdateNow)}>
          Try again
        </Button>
        <Button onClick={() => void run(api.openUpdatePage)}>Release page</Button>
      </>
    );
  } else if (update.postponed) {
    // "Later": the tray entry stays and brings this banner back.
    return null;
  } else if (update.auto_install) {
    text = (
      <>
        Deskmate {version} is available. It installs itself once the computer has not been used for
        five minutes; settings and pairing are kept.
      </>
    );
    buttons = (
      <Button kind="primary" onClick={() => void run(api.installUpdateNow)}>
        Install now
      </Button>
    );
  } else {
    text = (
      <>
        Deskmate {version} is available. Installing it restarts Deskmate; settings and pairing are
        kept.
      </>
    );
    buttons = (
      <>
        <Button kind="primary" onClick={() => void run(api.installUpdateNow)}>
          Install now
        </Button>
        <Button onClick={() => void run(api.postponeUpdate)}>Later</Button>
      </>
    );
  }

  return (
    <div className="mb-3 border border-hairline-strong rounded-md px-4 py-3 bg-panel">
      <div className="flex items-center justify-between gap-3">
        <p className="text-[13px]" aria-live="polite">
          {text}
        </p>
        {buttons && <div className="flex shrink-0 items-center gap-2">{buttons}</div>}
      </div>
      {update.phase === "downloading" && (
        <progress
          className="mt-3 block h-1 w-full accent-ink"
          aria-label={`Download of Deskmate ${update.version}`}
          max={update.total_bytes ?? undefined}
          value={update.total_bytes !== null ? update.downloaded_bytes : undefined}
        />
      )}
      {problem && <p className="mt-2 text-[12px] text-muted">{problem}</p>}
    </div>
  );
}
