CREATE TABLE `keys` (
	`id` text PRIMARY KEY NOT NULL,
	`key` text NOT NULL,
	`provider` text NOT NULL,
	`model_coolings` text,
	`total_cooling_seconds` integer DEFAULT 0 NOT NULL,
	`status` text DEFAULT 'active' NOT NULL,
	`created_at` integer DEFAULT (strftime('%s', 'now')) NOT NULL,
	`updated_at` integer DEFAULT (strftime('%s', 'now')) NOT NULL
);
--> statement-breakpoint
CREATE UNIQUE INDEX `provider_key_unq_idx` ON `keys` (`provider`,`key`);--> statement-breakpoint
CREATE INDEX `provider_status_created_at_idx` ON `keys` (`provider`,`status`,`created_at`);--> statement-breakpoint
CREATE INDEX `total_cooling_seconds_idx` ON `keys` (`total_cooling_seconds`);
